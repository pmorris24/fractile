#!/usr/bin/env swift

import Foundation

let paneSVG = """
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <g fill="#f4f6ff">
    <rect x="190" y="270" width="374" height="484" rx="76"/>
    <rect x="590" y="270" width="244" height="229" rx="58"/>
    <rect x="590" y="525" width="109" height="229" rx="40"/>
    <rect x="725" y="525" width="109" height="101" rx="30"/>
    <rect x="725" y="652" width="48" height="102" rx="16"/>
    <rect x="786" y="652" width="48" height="44" rx="12"/>
    <rect x="786" y="710" width="48" height="44" rx="12"/>
  </g>
</svg>
"""

let output = URL(fileURLWithPath: CommandLine.arguments.dropFirst().first ?? "app/AppIcon.icon/Assets")
try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)
try paneSVG.write(to: output.appendingPathComponent("Mark.svg"), atomically: true, encoding: .utf8)
