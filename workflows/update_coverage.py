import json
import os
import subprocess

report_path = "target/tarpaulin/coverage.json"
with open(report_path, "r") as f:
    data = json.load(f)

total_lines = 0
covered_lines = 0
files = data.get("files", [])

for file_data in files:
    file_path = os.path.join(*file_data.get("path", []))
    if (
        "/.cargo/" in file_path
        or "target/debug/build" in file_path
        or "/tests/" in file_path
        or file_path.endswith("/build.rs")
    ):
        continue

    trace_lines = file_data.get("traces", [])
    for trace in trace_lines:
        if "stats" in trace and "Line" in trace["stats"]:
            total_lines += 1
            if trace["stats"]["Line"] > 0:
                covered_lines += 1

percentage = 0
if total_lines > 0:
    percentage = round((covered_lines / total_lines) * 100)

print(f"Total lines: {total_lines}")
print(f"Covered lines: {covered_lines}")
print(f"Coverage percentage: {percentage}%")

output_dir = "badges_output/badges"
os.makedirs(output_dir, exist_ok=True)
badge_path = os.path.join(output_dir, "coverage.svg")

thresholds = {"40": "red", "60": "orange", "80": "yellow", "90": "green"}

badge_command = [
    "anybadge",
    "--label",
    "coverage",
    "--value",
    str(percentage),
    "--file",
    badge_path,
    "--suffix",
    "%",
]
threshold_args = [f"{k}={v}" for k, v in thresholds.items()]
badge_command.extend(threshold_args)
subprocess.run(badge_command)

print(f"Badge created at {badge_path}")
