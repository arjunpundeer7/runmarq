import type { Metadata } from "next";
import { docMetadata } from "@/content/docs";
import { PageTitle, Prose } from "@/components/Prose";

export const metadata: Metadata = docMetadata("/docs/eta-explained");

export default function EtaExplained() {
  return (
    <>
      <PageTitle title="How estimates work" />
      <Prose>
        <h2>What you see</h2>
        <ul>
          <li>
            <strong>An outlook</strong> that answers whether to wait: “Almost done” (likely within 2 min), “A few minutes” (within 10),
            “Time for something else” (most likely within half an hour) or “Long task”. While confidence is low, the outlook is the
            headline and the likely range sits under it. At medium or high confidence the range leads and the outlook moves under it.
          </li>
          <li>
            <strong>A likely range</strong>, such as “About 8–14 min”: the middle half of the predicted remaining time, from the 25th to
            the 75th percentile. The tick on the bar marks the median, P50.
          </li>
          <li>
            <strong>Finishing up</strong>, when the run shows signs of wrapping up: its plan is complete, or its tests pass with no
            edits since, and the agent has started no tool for 20 seconds. It appears only once your own earlier runs show that those
            signs usually meant the end within two minutes; in some workflows passing tests are a midpoint. The remaining time then
            comes from what followed the same signs before. If the agent starts another tool, the estimate returns to normal.
          </li>
          <li>
            <strong>How long it could take</strong>, such as “Could take up to 30 min”, when the 90th percentile reaches well past the
            likely range. The bar shows the full band from P10 to P90, with the likely part drawn stronger.
          </li>
          <li>
            <strong>Confidence</strong>: low, medium or high. It describes the evidence behind the estimate, not just its width.
          </li>
          <li>
            <strong>A reason</strong> whenever the estimate moves noticeably, for example “Estimate increased after repeated test
            failures.”
          </li>
        </ul>
        <p>
          The P10–P90 band targets 80% coverage. Once enough runs have finished, it is calibrated so that it does contain about 80% of
          outcomes on your machine, and Insights shows how often it actually did. Before that, it comes straight from the models and is
          labelled accordingly. The likely range is read from the same three percentiles on a log scale, each side with its own spread,
          so a long tail does not widen the range below the median.
        </p>

        <h2>What counts as one run</h2>
        <p>
          A run lasts from your prompt until the agent stops. A message you send while the agent is still working, or a notice that a
          background command it started has finished, continues the same run rather than starting a new one: it is one task to wait for.
          If the agent goes quiet without reporting an end and you prompt it again later, the earlier run is recorded as cancelled at its
          last activity, so the silence never counts as work. History recorded before this rule existed is repaired the same way when the
          models are trained.
        </p>

        <h2>The models</h2>
        <p>
          Four models run on your computer. Each joins the estimate once it has enough of your data to help, and Insights lists which ones
          are in use and why the others are not yet.
        </p>
        <ol>
          <li>
            <strong>History baseline.</strong> Total active time is treated as log-normal, a common shape for task durations: most runs are
            near a typical length and a few take much longer. It starts from deliberately wide built-in values for each kind of task and
            learns from your history, pooled across levels: all your runs, the same kind of task, the same agent, the same repository. Each
            level is pulled toward the one above in proportion to how little data it has, so three runs never outweigh three hundred.
            Once a run is under way, the remaining time is also read directly from your earlier runs that lasted at least as long, keeping
            their actual shape: when your history mixes quick answers with long tasks, the range covers both instead of predicting the
            gap between them.
            Effort level and model are learned as shared effects across all runs. Test failures, passing tests, plan progress and subagents
            then adjust it, each with a named reason.
          </li>
          <li>
            <strong>Phase simulation.</strong> A run moves between phases: planning, exploring, editing, testing, running commands,
            delegating. From your finished runs, runmarq learns how likely each next phase is (and how likely the run is to end) given the
            current phase and whether its last tool call failed, and how long each phase takes. It then simulates hundreds of possible
            continuations from where the run is now. Commands it has seen before, such as your test suite, use their own measured
            durations.
          </li>
          <li>
            <strong>Similar prompts.</strong> Each prompt is reduced to a salted, hashed word vector. The durations of earlier runs whose
            vectors are closest, and which lasted at least as long as the current run so far, give another estimate. You can turn this off
            in Settings.
          </li>
          <li>
            <strong>Learned model.</strong> After 100 finished runs, a gradient-boosted model is trained on moments from your runs: the
            task, effort, elapsed time, tool activity, test results, plan progress, token counts and working-tree changes, predicting the
            10th, 50th and 90th percentiles of remaining time directly.
          </li>
        </ol>
        <p>
          The models are combined by mixing their predictions rather than averaging their numbers, so when one expects a quick answer
          and another a long task, the range keeps both possibilities. How much each model counts is decided by testing them on your own
          recent runs as if they were live: models are fitted on older runs and asked to forecast newer ones at several moments, and each
          combination is scored on both its middle estimate and its range, every run counting once. Until there are many runs, the
          chosen weights stay close to the defaults, which favour the phase simulation because it starts from where the run actually
          is. A model that does not help gets no weight. When a run starts, its model is the one its session used last; the run&apos;s
          own model is known once its transcript has been read.
        </p>
        <p>
          Cancelled and interrupted runs are treated as censored: they tell the models the work would have lasted at least as long as it
          did, not that it finished then. Failed runs count at half weight.
        </p>

        <h2>Calibration</h2>
        <p>
          Those same out-of-sample forecasts show how far outcomes fell outside the predicted range. runmarq widens or narrows each end of
          the range by the amount that would have kept 90% of outcomes above P10 and 90% below P90, separately for estimates made early and
          late in a run. This is split-conformal prediction: as long as new runs resemble your recent ones, the calibrated range covers at
          least 80% of outcomes. It starts after about 34 finished runs and is refreshed as more finish.
        </p>

        <h2>Signals beyond hook events</h2>
        <ul>
          <li>
            <strong>Transcripts.</strong> Claude Code and Codex write session files on your computer. runmarq reads the numbers in them:
            how many model responses, output and thinking tokens, how full the context is, and compactions. Message text is not kept.
          </li>
          <li>
            <strong>Working tree.</strong> During a run, <code>git diff --numstat</code> counts files and lines changed since the run
            started. File names are not kept.
          </li>
          <li>
            <strong>Commands.</strong> A shell command is identified by a salted hash of its program and subcommand (for example
            “npm test”), never its arguments or output, so its typical duration can be learned.
          </li>
          <li>
            <strong>Effort.</strong> The reasoning effort the agent reports, such as low or high.
          </li>
        </ul>
        <p>
          Transcripts and working-tree changes can each be turned off in Settings → Estimation signals, as can the prompt comparison;
          command durations and effort are always used.
        </p>

        <h2>Why the estimate does not just count down</h2>
        <p>
          Subtracting elapsed time from a first guess goes wrong as soon as a run takes longer than typical. runmarq instead conditions on
          the run having survived this long. Early in a run, remaining time falls as time passes. When a run outlasts most similar runs, the
          estimate grows instead of reaching zero, and the reason “running longer than similar runs” appears.
        </p>
        <p>
          Waiting for your approval is not active time. While an agent waits for you, the estimate pauses and the card says so.
        </p>

        <h2>Task classification</h2>
        <p>
          When a prompt arrives, a deterministic classifier inside the hook process assigns a category and a size from its wording: words
          like “typo”, “refactor” or “migrate”, the number of listed steps, file mentions, and words that widen or narrow scope. No model is
          called and the text is discarded afterwards. When a run starts without a prompt event, the estimate uses only history and is
          labelled as such; with no history either, runmarq shows “Not enough information yet” instead of a number.
        </p>

        <h2>Confidence</h2>
        <ul>
          <li>
            <strong>Low</strong>: built-in values only, an unknown task, few similar runs, or a very wide range.
          </li>
          <li>
            <strong>Medium</strong>: at least eight comparable runs and a range that is not extremely wide.
          </li>
          <li>
            <strong>High</strong>: at least 25 comparable runs, a calibrated range that is reasonably narrow, and, once measured,
            interval coverage on your machine between 70% and 92%.
          </li>
        </ul>

        <h2>Measuring accuracy</h2>
        <p>
          After a run ends, its forecasts are compared with what happened. Insights reports median and mean error, how often the actual
          remaining time fell inside P10–P90, error at the start and after one, three and five minutes, error by agent and task type, and how
          much consecutive estimates move. A temporal holdout fits the models on your oldest runs and scores the initial estimate on the
          newest, with weights and calibration taken only from older runs, so it is not graded on data it has already seen.
        </p>
        <p>
          None of these figures is shown until at least ten finished runs can be evaluated; a handful of runs cannot support an accuracy
          figure.
        </p>

        <h2>Progress versus remaining time</h2>
        <p>
          The number of tool calls says little about how close a run is to done, so runmarq never turns it into a percentage. When the
          agent keeps a real plan (Claude&apos;s task list, Codex&apos;s plan), the card shows completed and total steps, and the time
          per completed step sets a target for the rest that the final estimate leans toward, more with every step done.
        </p>
        <p>
          The run&apos;s own pace matters too. Once it has made a few tool calls, the phase simulation compares its seconds per step with
          your usual and plays the rest of the run out at that speed, within limits, so a slow run is not forecast as if it were typical.
        </p>

        <h2>Checking it yourself</h2>
        <p>
          <code>runmarq eval</code> replays the runs recorded on your computer in the order they ended. It forecasts each one at several
          moments with models fitted only on the runs before it, as the live estimate would have been made, and reports how often the
          likely range held (ideally half the time), how often the full range held (ideally 80%), how often the outlook was right, the
          typical error, and how wide the ranges were. Add <code>--json</code> for the raw figures.
        </p>
        <p>
          All of this is computed locally. Estimating a run makes no network requests and uses none of your agent&apos;s model usage.
        </p>
      </Prose>
    </>
  );
}
