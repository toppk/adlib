#!/usr/bin/env python3
"""
Parse adlib debug logs and display a clean summary of transcription events.

Usage:
    ./scripts/parse_debug_log.py scripts/sample.log.txt
    ./scripts/parse_debug_log.py scripts/sample.log.txt --full
    ./scripts/parse_debug_log.py scripts/sample.log.txt --json

To generate debug logs, run adlib with: adlib -vv 2> debug.log
"""

import argparse
import json
import re
import signal
import sys
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

# Handle broken pipe gracefully (e.g., when piping to head)
signal.signal(signal.SIGPIPE, signal.SIG_DFL)


@dataclass
class Event:
    timestamp: datetime
    event_type: str
    text: str = ""
    extra: dict = None

    def to_dict(self):
        d = {
            "timestamp": self.timestamp.isoformat(),
            "event_type": self.event_type,
        }
        if self.text:
            d["text"] = self.text
        if self.extra:
            d.update(self.extra)
        return d


# Regex patterns for parsing log lines
PATTERNS = {
    "live": re.compile(r"\[LIVE\] '(.*)'$"),
    "commit": re.compile(r"\[COMMIT\] '(.*)' \((\d+) chars\)$"),
    "pause": re.compile(r"\[PAUSE\] Running final transcription on (\d+) samples \(trimmed (\d+) silence samples\)"),
    "segment": re.compile(r"\[SEGMENT (\d+)\] text='(.*)', empty=(true|false), hallucination=(true|false)"),
    "segments": re.compile(r"\[SEGMENTS\] num_segments=(\d+)"),
    "silence": re.compile(r"\[SILENCE\] count=(\d+)/(\d+), rms=([\d.]+), threshold=([\d.]+)"),
    "speech": re.compile(r"\[SPEECH\] Detected, resetting silence count from (\d+)"),
}

TIMESTAMP_RE = re.compile(r"^\[(\d{4}-\d{2}-\d{2}T[\d:.]+Z)\s+\w+")


def parse_timestamp(line: str) -> datetime | None:
    match = TIMESTAMP_RE.match(line)
    if match:
        ts_str = match.group(1)
        # Handle milliseconds
        try:
            return datetime.fromisoformat(ts_str.replace("Z", "+00:00"))
        except ValueError:
            return None
    return None


def parse_line(line: str) -> Event | None:
    ts = parse_timestamp(line)
    if not ts:
        return None

    for event_type, pattern in PATTERNS.items():
        match = pattern.search(line)
        if match:
            if event_type == "live":
                return Event(ts, "LIVE", text=match.group(1))
            elif event_type == "commit":
                return Event(ts, "COMMIT", text=match.group(1), extra={"chars": int(match.group(2))})
            elif event_type == "pause":
                return Event(ts, "PAUSE", extra={
                    "samples": int(match.group(1)),
                    "trimmed_samples": int(match.group(2))
                })
            elif event_type == "segment":
                return Event(ts, "SEGMENT", text=match.group(2), extra={
                    "index": int(match.group(1)),
                    "empty": match.group(3) == "true",
                    "hallucination": match.group(4) == "true"
                })
            elif event_type == "segments":
                return Event(ts, "SEGMENTS", extra={"num_segments": int(match.group(1))})
            elif event_type == "silence":
                return Event(ts, "SILENCE", extra={
                    "count": int(match.group(1)),
                    "max_count": int(match.group(2)),
                    "rms": float(match.group(3)),
                    "threshold": float(match.group(4))
                })
            elif event_type == "speech":
                return Event(ts, "SPEECH", extra={"reset_from": int(match.group(1))})
    return None


def parse_log(filepath: Path) -> list[Event]:
    events = []
    with open(filepath, "r") as f:
        for line in f:
            event = parse_line(line.strip())
            if event:
                events.append(event)
    return events


def print_summary(events: list[Event]):
    """Print a concise summary of commits only."""
    commits = [e for e in events if e.event_type == "COMMIT"]
    pauses = [e for e in events if e.event_type == "PAUSE"]
    hallucinations = [e for e in events if e.event_type == "SEGMENT" and e.extra and e.extra.get("hallucination")]

    print(f"=== Log Summary ===")
    print(f"Total events: {len(events)}")
    print(f"Commits: {len(commits)}")
    print(f"Pauses: {len(pauses)}")
    print(f"Hallucination rejections: {len(hallucinations)}")
    print()

    print("=== Committed Segments ===")
    for i, commit in enumerate(commits, 1):
        time_str = commit.timestamp.strftime("%H:%M:%S")
        chars = commit.extra.get("chars", "?") if commit.extra else "?"
        print(f"{i}. [{time_str}] ({chars} chars)")
        print(f"   {commit.text}")
        print()

    if hallucinations:
        print("=== Hallucination Rejections ===")
        for h in hallucinations:
            time_str = h.timestamp.strftime("%H:%M:%S")
            print(f"[{time_str}] '{h.text}'")


def print_full(events: list[Event]):
    """Print all events in a readable timeline."""
    for event in events:
        time_str = event.timestamp.strftime("%H:%M:%S.%f")[:-3]

        if event.event_type == "LIVE":
            print(f"[{time_str}] LIVE: {event.text}")
        elif event.event_type == "COMMIT":
            chars = event.extra.get("chars", "?") if event.extra else "?"
            print(f"[{time_str}] COMMIT ({chars} chars): {event.text}")
        elif event.event_type == "PAUSE":
            samples = event.extra.get("samples", "?") if event.extra else "?"
            print(f"[{time_str}] PAUSE: {samples} samples")
        elif event.event_type == "SEGMENT":
            h = event.extra.get("hallucination", False) if event.extra else False
            marker = " [HALLUCINATION]" if h else ""
            print(f"[{time_str}] SEGMENT{marker}: {event.text}")
        elif event.event_type == "SILENCE":
            if event.extra:
                print(f"[{time_str}] SILENCE: {event.extra['count']}/{event.extra['max_count']}")
        elif event.event_type == "SPEECH":
            print(f"[{time_str}] SPEECH detected")


def print_json(events: list[Event]):
    """Print events as JSON Lines."""
    for event in events:
        print(json.dumps(event.to_dict()))


def main():
    parser = argparse.ArgumentParser(description="Parse adlib debug logs")
    parser.add_argument("logfile", type=Path, help="Path to debug log file")
    parser.add_argument("--full", action="store_true", help="Show all events in timeline")
    parser.add_argument("--json", action="store_true", help="Output as JSON Lines")
    args = parser.parse_args()

    if not args.logfile.exists():
        print(f"Error: {args.logfile} not found", file=sys.stderr)
        sys.exit(1)

    events = parse_log(args.logfile)

    if args.json:
        print_json(events)
    elif args.full:
        print_full(events)
    else:
        print_summary(events)


if __name__ == "__main__":
    main()
