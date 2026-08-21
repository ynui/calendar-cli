# calendar-cli

A terminal-based calendar app with Google Calendar sync, local event storage, and an interactive TUI.

## Features

- **Month calendar view** with event dots and keyboard navigation
- **Google Calendar sync** — OAuth sign-in, bidirectional event management
- **Local calendar** — works offline, stores events in JSON
- **Event management** — create, edit, delete events with a form popup
- **Live clock** — current date/time displayed in the menu bar
- **Event search** — filter events by title or description with `/`
- **Customizable themes** — Default, Light, Ocean
- **First day of week** — Monday or Sunday
- **RTL support** — Hebrew and Arabic text handled correctly
- **`cal` shell command** — optional registration to launch from anywhere
- **Settings panel** — manage accounts, preferences, and shell integration

## Screenshot

```
╭── File    Calendar    Account    Help           Wed May 27 2026  14:30:05 ──╮
│                                                                             │
│  ┌────────── ◄  May 2026  ► ────────┐  ┌─── Wed, May 27 (3) ──────────────┐ │
│  │ Mo Tu We Th Fr Sa Su             │  │  09:00 Standup                   │ │
│  │        1  2  3  4  5             │  │  12:00 Lunch                     │ │
│  │  6  7  8  9 10 11 12             │  │  15:30 Review                    │ │
│  │ 13 14 15 16 17 18 19             │  │                                  │ │
│  │ 20 21 22 23 24 25·26             │  │                                  │ │
│  │ 27·28 29 30 31                   │  │                                  │ │
│  └──────────────────────────────────┘  └──────────────────────────────────┘ │
│  [Arrows] Days  [/] Search  [Enter] Options  [Tab] Focus  [Esc] Quit        │
╰─────────────────────────────────────────────────────────────────────────────╯
```

## Installation

### Homebrew (recommended)

```bash
brew install ynui/tap/calendar-cli
```

Universal binary — Apple Silicon and Intel.

### From source

```bash
cargo install --git https://github.com/ynui/calendar-cli
```

## Usage

Run from your terminal:

```bash
calendar-cli        # or the short alias: ccal
```

Register a `cal` command (optional, via Settings > Shell) to launch with just `cal`.

## Keybindings

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Cycle focus: Menu Bar → Calendar → Events |
| `Arrow keys` | Navigate days / menu items |
| `PageUp` / `[` | Previous month |
| `PageDown` / `]` | Next month |
| `Enter` | Open menu / context menu / confirm |
| `Esc` | Close menu / quit |
| `/` | Search events |

### Search mode

| Key | Action |
|---|---|
| Type | Filter events by title or description |
| `Up` / `Down` | Navigate search results |
| `Enter` | Jump to the selected event's date |
| `Esc` | Cancel search |

## Google Calendar Setup

1. Go to [Google Cloud Console](https://console.cloud.google.com/)
2. Create a project, enable the **Google Calendar API**
3. Create OAuth 2.0 credentials (Desktop app type)
4. Download the JSON and place it at `credentials.json` in the config directory (macOS: `~/Library/Application Support/calendar-cli/`, Linux: `~/.config/calendar-cli/`)

Sign in via **Account > Sign In to Google** in the menu bar. Your browser will open for authorization.

Tip: if `credentials.json` is missing, choosing **Sign In** opens an in-app setup screen showing the exact path and an example file.

## Settings

Access via **File > Settings** (or menu bar).

| Item | Action |
|---|---|
| **Local Storage** | Always active — stores events locally |
| **Google Calendar** | Sign in/out |
| **Start week on** | Toggle Monday / Sunday |
| **Theme** | Cycle Default → Light → Ocean |
| **cal command** | Register/unregister shell command |

## Themes

| Theme | Description |
|---|---|
| **Default** | Dark background, blue selection, cyan accents |
| **Light** | Light background, black accents |
| **Ocean** | Deep blue background, cyan accents |

## Data Storage

Config directory: `~/Library/Application Support/calendar-cli/` (macOS) or `~/.config/calendar-cli/` (Linux)

| File | Purpose |
|---|---|
| `credentials.json` | Google OAuth client ID/secret |
| `token.json` | Google OAuth access/refresh token |
| `events.json` | Local event store |
| `settings.json` | Preferences (theme, week start) |

## Releasing

```bash
./bump-version patch   # or minor / major / X.Y.Z
git push               # CI builds the universal binary, publishes the release, updates Homebrew
```

## Architecture

- **Backend trait** (`CalendarBackend`) — abstract over Google Calendar and local storage
- **Auth flow** — embedded OAuth 2.0 with localhost TCP redirect listener
- **Month caching** — events loaded per month, filtered client-side for individual days
- **RTL support** — uses `unicode-bidi` for correct Hebrew/Arabic rendering
