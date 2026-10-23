export interface ChangelogEntry {
  version: string;
  date: string;
  status: "released" | "unreleased";
  items: string[];
}

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "0.1.0",
    date: "2026-10-22",
    status: "unreleased",
    items: [
      "Local collector with authenticated loopback API, single-instance lock, crash recovery and a bounded offline spool.",
      "Run state engine: running, waiting for you, finished, failed, cancelled and unknown, with separate active and waiting time.",
      "ETA estimator: a log-normal survival baseline learned hierarchically from local history, a semi-Markov phase simulation, similar-prompt lookup over salted word vectors, and a quantile gradient-boosted model after 100 runs, combined with weights chosen on your own recent runs.",
      "Split-conformal calibration of the P10–P90 range, per forecast age, once about 34 runs have finished.",
      "Estimates lead with the likely range (25th to 75th percentile) and add “could take up to …” only when the tail reaches well past it.",
      "An outlook headline (almost done, a few minutes, time for something else, long task) leads while confidence is low; the range leads once it is medium or high.",
      "“Finishing up” appears when the plan is complete or tests pass after the last edit and the agent stops starting tools, with remaining time learned from earlier runs at the same point.",
      "Plan progress sets a target for the final estimate, and the phase simulation plays the rest of a run out at its own pace.",
      "Plan progress from Claude Code's task list (TaskCreate and TaskUpdate), as well as its older to-do list.",
      "Estimates combine the models by mixing their predictions, so a run that could be a quick answer or a long task keeps both in its range; model weights are chosen by a proper forecast score and stay close to the defaults until many runs exist.",
      "Once a run is under way, the earlier runs that outlasted it are read with their actual shape rather than one average.",
      "“Finishing up” appears only after 20 quiet seconds, and only once your own history shows those signs really meant the end.",
      "A new run starts with its session's model already known; runmarq eval no longer uses information a live estimate would not have had.",
      "runmarq wait shows the run's progress on the terminal tab and taskbar in Windows Terminal, ConEmu, Ghostty and WezTerm.",
      "The desktop app offers to start at login during setup, switched on.",
      "The desktop app can make the runmarq command available in new terminals: on the user's Path on Windows, through ~/.local/bin on macOS and Linux.",
      "Windows installer for the current user, without administrator rights; the app, installer and tray carry the current logo.",
      "A run whose agent is closed mid-run, for example with its terminal window, ends as cancelled within seconds instead of waiting 20 minutes.",
      "runmarq eval replays recorded runs in order to measure how often the likely range, the full range and the outlook held.",
      "Stuck detection: a warning and a notification when the same tool keeps failing, a command runs far longer than usual, or a run is long past its estimate with no activity.",
      "Usage page: output tokens, responses, runs and working time per agent for today, the last 7 days and the current 5-hour window, with an optional budget.",
      "runmarq wait blocks until a run ends and exits with its outcome; runmarq statusline prints one line for prompts, tmux and Claude Code.",
      "An optional sound with notifications.",
      "Once a run is under way, the history baseline reads remaining time from earlier runs that lasted at least as long.",
      "Prompts that arrive while the agent is still working, such as messages typed mid-turn or background-task notices, continue the run instead of splitting it. Cut-off runs end at their last activity, and history recorded before this is repaired at training time.",
      "Signals beyond hook events: token counts and context size from Claude Code and Codex transcripts, working-tree changes from git, learned durations for shell commands, and reported effort level. Transcripts, git and prompt comparison can each be turned off.",
      "Accuracy evaluation with interval coverage, error by forecast age and a temporal holdout, withheld until enough runs exist.",
      "Integrations for Claude Code (hooks), Codex (hooks and the runmarq run wrapper) and Cursor (observer-only hooks), each with setup preview, backup, verification and clean removal.",
      "Desktop app with onboarding, overview, run detail, history with CSV export, insights, settings, tray, notifications and a compact panel.",
      "runmarq command: status, doctor, history, export, forget, integrations and collector control; status, doctor, history and the integration commands print JSON.",
    ],
  },
];
