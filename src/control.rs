use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::{OpenOptionsExt, PermissionsExt},
        net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::mpsc::{Sender, channel},
    thread,
    time::Duration,
};

use crate::{
    DisplaySelector, DisplayTarget, ResizeDirection, WorkspaceId, WorkspaceStep,
    app::{Action, WorkerAction},
    deep_link::{self, DeepLink},
    scratchpad::ScratchpadCommand,
};

const MAX_REQUEST_BYTES: u64 = 4 * 1024;
const REQUEST_READ_TIMEOUT: Duration = Duration::from_millis(250);
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(6);

pub const COMMAND_USAGE: &str = "status|debug|settings|open TARGET|open-settings|open-accessibility|toggle-launch-at-login|switch WORKSPACE|switch next|prev|back-and-forth|move-workspace [next|prev|DISPLAY]|move WORKSPACE [--follow]|move next|prev [--follow]|move-tab WORKSPACE [--follow]|move-app ID WORKSPACE|focus DIR|focus next|focus prev|move-direction DIR|layout|fullscreen|float|pin|resize shrink|resize grow|resize width|height POINTS|balance|scratchpad toggle|scratchpad mark|pause|resume|retile|wrong|restart|quit";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Command {
    Status,
    Debug,
    /// What the settings window shows that only the agent knows.
    Settings,
    /// Open Settings to a validated `fractile://` deep-link target.
    Open(DeepLink),
    App(crate::app::Action),
    Queue(WorkerAction),
    Pause,
    Resume,
    Restart,
    Quit,
}

pub(crate) struct Request {
    pub(crate) command: Command,
    response: Sender<Result<String, String>>,
}

impl Request {
    pub(crate) fn respond(self, response: Result<String, String>) {
        // Client timed out or closed its socket before the reply was ready.
        drop(self.response.send(response));
    }
}

pub(crate) struct Server {
    path: PathBuf,
    _lock: File,
}

impl Drop for Server {
    fn drop(&mut self) {
        drop(fs::remove_file(&self.path));
    }
}

pub(crate) fn listen(requests: Sender<Request>) -> Result<Server, String> {
    let path = socket_path()?;
    listen_at(path, requests)
}

fn listen_at(path: PathBuf, requests: Sender<Request>) -> Result<Server, String> {
    let parent = path.parent().ok_or("control socket path has no parent")?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create control socket directory: {error}"))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("failed to secure control socket directory: {error}"))?;
    let lock_path = path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(&lock_path)
        .map_err(|error| format!("failed to open control socket lock: {error}"))?;
    lock.try_lock()
        .map_err(|_| "another Fractile agent is already starting or running".to_string())?;
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("failed to secure control socket lock: {error}"))?;
    if UnixStream::connect(&path).is_ok() {
        return Err("another Fractile agent is already running".into());
    }
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("failed to remove stale control socket: {error}")),
    }
    let listener = UnixListener::bind(&path)
        .map_err(|error| format!("failed to bind control socket: {error}"))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("failed to secure control socket: {error}"))?;
    if let Err(error) = thread::Builder::new()
        .name("fractile-control".into())
        .spawn(move || serve(listener, requests))
    {
        drop(fs::remove_file(&path));
        return Err(format!("failed to start control server: {error}"));
    }
    Ok(Server { path, _lock: lock })
}

pub fn send(arguments: &[String]) -> Result<String, String> {
    let command = arguments.join(" ");
    parse_line(&command)?;
    let path = socket_path()?;
    let mut stream =
        UnixStream::connect(&path).map_err(|_| "Fractile agent is not running".to_string())?;
    stream
        .set_write_timeout(Some(CLIENT_TIMEOUT))
        .map_err(|error| format!("failed to configure control command: {error}"))?;
    stream
        .set_read_timeout(Some(CLIENT_TIMEOUT))
        .map_err(|error| format!("failed to configure control response: {error}"))?;
    writeln!(stream, "{command}")
        .map_err(|error| format!("failed to send control command: {error}"))?;
    stream
        .shutdown(std::net::Shutdown::Write)
        .map_err(|error| format!("failed to finish control command: {error}"))?;
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|error| format!("failed to read control response: {error}"))?;
    if let Some(value) = response.strip_prefix("ok\n") {
        Ok(value.trim_end().to_string())
    } else if let Some(error) = response.strip_prefix("error\n") {
        Err(error.trim_end().to_string())
    } else {
        Err("Fractile returned an invalid control response".into())
    }
}

fn serve(listener: UnixListener, requests: Sender<Request>) {
    for connection in listener.incoming() {
        let Ok(mut stream) = connection else {
            continue;
        };
        drop(stream.set_read_timeout(Some(REQUEST_READ_TIMEOUT)));
        drop(stream.set_write_timeout(Some(REQUEST_READ_TIMEOUT)));
        let response = read_command(&stream).and_then(|line| {
            let command = parse_line(line.trim())?;
            let (response, receiver) = channel();
            requests
                .send(Request { command, response })
                .map_err(|_| "Fractile agent is stopping".to_string())?;
            receiver
                .recv_timeout(RESPONSE_TIMEOUT)
                .map_err(|_| "Fractile did not respond to the control command".to_string())?
        });
        let (prefix, message) = match response {
            Ok(message) => ("ok", message),
            Err(error) => ("error", error),
        };
        drop(writeln!(stream, "{prefix}\n{message}"));
    }
}

fn read_command(reader: impl Read) -> Result<String, String> {
    let mut bytes = Vec::new();
    BufReader::new(reader)
        .take(MAX_REQUEST_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|error| format!("failed to read control command: {error}"))?;
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err("control command is too large".into());
    }
    if !bytes.ends_with(b"\n") {
        return Err("control command must end with a newline".into());
    }
    String::from_utf8(bytes).map_err(|_| "control command is not valid UTF-8".into())
}

fn socket_path() -> Result<PathBuf, String> {
    crate::config::application_support_path("control.sock").ok_or("HOME is unavailable".into())
}

fn parse_line(line: &str) -> Result<Command, String> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix("move-workspace")
        && (rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        let target = match rest.trim() {
            "" | "next" => DisplayTarget::Next,
            "prev" => DisplayTarget::Prev,
            display => DisplayTarget::Selector(DisplaySelector::parse(display)),
        };
        return Ok(Command::Queue(WorkerAction::MoveWorkspaceToDisplay(target)));
    }
    let parts = trimmed.split_whitespace().collect::<Vec<_>>();
    let action = match parts.as_slice() {
        ["status"] => return Ok(Command::Status),
        ["debug"] => return Ok(Command::Debug),
        ["settings"] => return Ok(Command::Settings),
        ["open", target] => {
            let link = deep_link::parse_input(target).map_err(|error| error.to_string())?;
            return Ok(Command::Open(link));
        }
        ["open-settings"] => return Ok(Command::App(Action::OpenSettings)),
        ["open-accessibility"] => return Ok(Command::App(Action::OpenAccessibility)),
        ["toggle-launch-at-login"] => return Ok(Command::App(Action::ToggleLaunchAtLogin)),
        ["pause"] => return Ok(Command::Pause),
        ["resume"] => return Ok(Command::Resume),
        ["restart"] => return Ok(Command::Restart),
        ["quit"] => return Ok(Command::Quit),
        ["switch", target] => match WorkspaceStep::parse(target) {
            Some(step) => WorkerAction::StepWorkspace(step),
            None => WorkerAction::SwitchWorkspace(WorkspaceId::parse(target)?),
        },
        ["back-and-forth"] => WorkerAction::WorkspaceBackAndForth,
        ["move", target] => match WorkspaceStep::parse(target) {
            Some(step) => WorkerAction::MoveFocusedStep {
                step,
                follow: false,
            },
            None => WorkerAction::MoveFocused(WorkspaceId::parse(target)?),
        },
        ["move", target, "--follow"] | ["move", "--follow", target] => {
            match WorkspaceStep::parse(target) {
                Some(step) => WorkerAction::MoveFocusedStep { step, follow: true },
                None => WorkerAction::MoveFocusedFollow(WorkspaceId::parse(target)?),
            }
        }
        ["move-tab", workspace] => WorkerAction::MoveTab {
            target: WorkspaceId::parse(workspace)?,
            follow: false,
        },
        ["move-tab", workspace, "--follow"] | ["move-tab", "--follow", workspace] => {
            WorkerAction::MoveTab {
                target: WorkspaceId::parse(workspace)?,
                follow: true,
            }
        }
        ["move-app", bundle_id, workspace] => {
            WorkerAction::MoveApp((*bundle_id).into(), WorkspaceId::parse(workspace)?)
        }
        ["focus", "next"] => WorkerAction::CycleFocus,
        ["focus", "prev"] => WorkerAction::CycleFocusPrevious,
        ["focus", direction] => WorkerAction::Focus(direction.parse()?),
        ["move-direction", direction] => WorkerAction::Reorder(direction.parse()?),
        ["layout"] => WorkerAction::ToggleLayout,
        ["fullscreen"] => WorkerAction::ToggleFullscreenLayout,
        ["float"] => WorkerAction::ToggleFloat,
        ["pin"] => WorkerAction::TogglePin,
        ["resize", "shrink"] => WorkerAction::Resize(ResizeDirection::Shrink),
        ["resize", "grow"] => WorkerAction::Resize(ResizeDirection::Grow),
        ["resize", dimension, points] => {
            let (axis, points) = crate::parse_resize_to(dimension, points)?;
            WorkerAction::ResizeTo(axis, points)
        }
        ["balance"] => WorkerAction::Balance,
        ["scratchpad", command] => WorkerAction::Scratchpad(
            ScratchpadCommand::parse(command).ok_or("expected scratchpad toggle|mark")?,
        ),
        ["retile"] => WorkerAction::Retile,
        ["wrong"] => WorkerAction::MarkWrong,
        _ => return Err(format!("expected {COMMAND_USAGE}")),
    };
    Ok(Command::Queue(action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Direction;

    #[test]
    fn parses_control_commands() {
        let queued = |line| match parse_line(line).unwrap() {
            Command::Queue(action) => action,
            command => panic!("{line:?} parsed as {command:?}"),
        };
        assert_eq!(parse_line("status").unwrap(), Command::Status);
        assert_eq!(queued("wrong"), WorkerAction::MarkWrong);
        assert_eq!(
            queued("back-and-forth"),
            WorkerAction::WorkspaceBackAndForth
        );
        assert_eq!(
            queued("move-workspace"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next)
        );
        assert_eq!(
            queued("move-workspace next"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Next)
        );
        assert_eq!(
            queued("move-workspace prev"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Prev)
        );
        assert_eq!(
            queued("move-workspace main"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Selector(DisplaySelector::Main))
        );
        assert_eq!(
            queued("move-workspace secondary"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Selector(
                DisplaySelector::Secondary
            ))
        );
        assert_eq!(
            queued("move-workspace Studio Display"),
            WorkerAction::MoveWorkspaceToDisplay(DisplayTarget::Selector(
                DisplaySelector::Preferred("Studio Display".into())
            ))
        );
        assert!(parse_line("move-workspace-extra").is_err());
        assert_eq!(
            queued("switch 10"),
            WorkerAction::SwitchWorkspace(WorkspaceId::new(10).unwrap())
        );
        assert!(parse_line("move 11").is_err());
        assert_eq!(
            queued("switch next"),
            WorkerAction::StepWorkspace(WorkspaceStep::Next)
        );
        assert_eq!(
            queued("switch prev"),
            WorkerAction::StepWorkspace(WorkspaceStep::Prev)
        );
        assert_eq!(
            queued("move next"),
            WorkerAction::MoveFocusedStep {
                step: WorkspaceStep::Next,
                follow: false,
            }
        );
        assert_eq!(
            queued("move --follow prev"),
            WorkerAction::MoveFocusedStep {
                step: WorkspaceStep::Prev,
                follow: true,
            }
        );
        assert!(parse_line("switch previous").is_err());
        assert!(parse_line("move-tab next").is_err());
        assert_eq!(
            queued("move 2 --follow"),
            WorkerAction::MoveFocusedFollow(WorkspaceId::new(2).unwrap())
        );
        assert_eq!(
            queued("move --follow D"),
            WorkerAction::MoveFocusedFollow(WorkspaceId::named('D').unwrap())
        );
        assert_eq!(
            queued("switch D"),
            WorkerAction::SwitchWorkspace(WorkspaceId::named('D').unwrap())
        );
        assert_eq!(
            queued("move-tab 2"),
            WorkerAction::MoveTab {
                target: WorkspaceId::new(2).unwrap(),
                follow: false,
            }
        );
        assert_eq!(
            queued("move-tab 2 --follow"),
            WorkerAction::MoveTab {
                target: WorkspaceId::TWO,
                follow: true,
            }
        );
        assert_eq!(
            queued("move-tab --follow D"),
            WorkerAction::MoveTab {
                target: WorkspaceId::named('D').unwrap(),
                follow: true,
            }
        );
        assert!(parse_line("move-tab 11").is_err());
        assert!(parse_line("move-tab").is_err());
        assert_eq!(
            queued("move-app com.example.App 3"),
            WorkerAction::MoveApp("com.example.App".into(), WorkspaceId::new(3).unwrap())
        );
        assert!(parse_line("resume now").is_err());
        assert_eq!(parse_line("restart").unwrap(), Command::Restart);
        assert_eq!(queued("focus left"), WorkerAction::Focus(Direction::Left));
        assert_eq!(
            queued("move-direction right"),
            WorkerAction::Reorder(Direction::Right)
        );
        assert_eq!(queued("layout"), WorkerAction::ToggleLayout);
        assert_eq!(
            queued("resize shrink"),
            WorkerAction::Resize(ResizeDirection::Shrink)
        );
        assert_eq!(
            queued("resize grow"),
            WorkerAction::Resize(ResizeDirection::Grow)
        );
        assert_eq!(
            queued("resize width 1200"),
            WorkerAction::ResizeTo(crate::Axis::Horizontal, 1200)
        );
        assert_eq!(
            queued("resize height 600"),
            WorkerAction::ResizeTo(crate::Axis::Vertical, 600)
        );
        assert!(parse_line("resize width").is_err());
        assert!(parse_line("resize width 0").is_err());
        assert!(parse_line("resize width wide").is_err());
        assert!(parse_line("resize depth 600").is_err());
        assert_eq!(queued("balance"), WorkerAction::Balance);
        assert!(parse_line("balance now").is_err());
        assert_eq!(queued("focus next"), WorkerAction::CycleFocus);
        assert_eq!(queued("focus prev"), WorkerAction::CycleFocusPrevious);
        assert_eq!(queued("fullscreen"), WorkerAction::ToggleFullscreenLayout);
        assert_eq!(queued("float"), WorkerAction::ToggleFloat);
        assert_eq!(queued("pin"), WorkerAction::TogglePin);
        assert_eq!(
            queued("scratchpad toggle"),
            WorkerAction::Scratchpad(ScratchpadCommand::Toggle)
        );
        assert_eq!(
            queued("scratchpad mark"),
            WorkerAction::Scratchpad(ScratchpadCommand::Mark)
        );
        assert!(parse_line("scratchpad").is_err());
        assert!(parse_line("scratchpad show").is_err());
        assert_eq!(
            parse_line("open fractile://settings/workspaces").unwrap(),
            Command::Open(DeepLink::Settings {
                pane: Some(deep_link::DeepLinkPane::Workspaces),
                unknown_pane: None,
                focus: None,
            })
        );
        assert_eq!(
            parse_line("open labs/scrolling-strip").unwrap(),
            Command::Open(DeepLink::Labs {
                slug: Some("scrolling-strip".into()),
            })
        );
        assert!(parse_line("open https://evil.example/labs").is_err());
        assert!(parse_line("open labs/%2e%2e").is_err());
    }

    #[test]
    fn control_socket_lock_prevents_a_second_server_from_removing_the_first_socket() {
        let path = temporary_socket_path("lock");
        let (requests, _receiver) = channel();
        let server = listen_at(path.clone(), requests).unwrap();
        let (requests, _receiver) = channel();

        assert!(listen_at(path.clone(), requests).is_err());
        assert!(UnixStream::connect(&path).is_ok());
        let lock_path = path.with_extension("lock");
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for secured in [&path, &lock_path] {
            assert_eq!(
                fs::metadata(secured).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        drop(server);
        drop(fs::remove_file(lock_path));
        drop(fs::remove_dir(path.parent().unwrap()));
    }

    #[test]
    fn partial_request_read_is_bounded() {
        let (mut client, server) = UnixStream::pair().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        client.write_all(b"sta").unwrap();
        let error = read_command(server).unwrap_err();

        assert!(error.contains("failed to read control command"));
    }

    #[test]
    fn command_input_requires_a_newline_and_has_a_fixed_limit() {
        assert_eq!(read_command(&b"status\n"[..]).unwrap(), "status\n");
        assert_eq!(
            read_command(&b"status"[..]).unwrap_err(),
            "control command must end with a newline"
        );
        let oversized = vec![b'x'; usize::try_from(MAX_REQUEST_BYTES + 1).unwrap()];
        assert_eq!(
            read_command(oversized.as_slice()).unwrap_err(),
            "control command is too large"
        );
    }

    fn temporary_socket_path(label: &str) -> PathBuf {
        let nonce = crate::config::now().as_nanos();
        std::env::temp_dir()
            .join(format!("fractile-{label}-{}-{nonce}", std::process::id()))
            .join("control.sock")
    }
}
