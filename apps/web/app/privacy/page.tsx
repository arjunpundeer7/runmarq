import type { Metadata } from "next";
import { ContentPage, PageTitle, Prose } from "@/components/Prose";
import { earlyAccessConfigured, pageMetadata, site } from "@/lib/site";

export const metadata: Metadata = pageMetadata("/privacy", "Privacy", "What runmarq records on your computer, what it never stores, and how to delete it.");

export default function Privacy() {
  return (
    <ContentPage>
      <PageTitle title="Privacy" />
      <Prose>
        <h2>The app</h2>
        <p>
          The desktop app and the <code>runmarq</code> command run on your computer and store data only there, in runmarq&apos;s data
          folder. They do not need an account, make no network requests to compute estimates, and include no analytics or crash reporting.
        </p>
        <h3>Stored on your computer</h3>
        <ul>
          <li>When each run started, waited and ended, and how it ended.</li>
          <li>For each event: its type, time, tool name (such as “Bash” or “Edit”), activity category and duration.</li>
          <li>A task category and size estimated from the prompt, and a salted hash of the prompt used only to recognise repeats.</li>
          <li>
            Unless you turn it off: a salted, hashed word vector of the prompt, used to find similar earlier runs. The prompt cannot be
            read back from it.
          </li>
          <li>The effort level and permission mode the agent reports.</li>
          <li>
            Unless you turn it off: numbers read from the agent&apos;s own transcript on your computer (response counts, token counts and
            context size), and how many files and lines changed in the repository during the run, from <code>git diff --numstat</code>.
          </li>
          <li>
            For shell commands: a salted hash of the program and subcommand (such as “npm test”) and how long it took.
          </li>
          <li>A salted hash of the repository&apos;s location, and its folder name if you allow it (on by default; off in Settings).</li>
          <li>The model name, when the agent reports one.</li>
          <li>Forecasts, and how they compared with what happened.</li>
          <li>Your settings, including any usage budget you set, and a log of which notifications were shown.</li>
          <li>If you turn it on: a short title from the first line of each prompt, with secret-looking text removed.</li>
        </ul>
        <h3>Never stored</h3>
        <ul>
          <li>Prompt text (apart from the optional title above).</li>
          <li>Source code or file contents.</li>
          <li>Commands, their arguments, or their output.</li>
          <li>Transcript text, the transcript&apos;s location, the working folder&apos;s path, or the names of changed files.</li>
          <li>Environment variables, API keys or tokens.</li>
        </ul>
        <p>
          Estimates are computed entirely on your computer and use none of your agent&apos;s model usage. Prompt text is read only inside
          the hook process, to classify the task, and is discarded when that process exits a few milliseconds later. If the collector is
          not running, the already-minimised events wait in a small file on disk and are deleted once delivered.
        </p>
        <p>
          The Usage page and stuck warnings are worked out from the counts and timings listed above; nothing extra is collected for them.
        </p>
        <h3>Your control</h3>
        <ul>
          <li>Delete history at any time in Settings → Data, or with <code>runmarq forget</code>.</li>
          <li>Set automatic deletion after 30, 90 or 365 days.</li>
          <li>Export your history as CSV, which contains timing data only.</li>
          <li>Removing an integration removes only the hook entries runmarq added to that agent&apos;s configuration.</li>
        </ul>
        <h2>This website</h2>
        <p>
          This site counts visits with Vercel Web Analytics: the page, the referring site, the browser, operating system and device
          type, and the country. It sets no cookies and keeps no personal identifiers, and the site has no other trackers. The server
          may keep standard request logs (address, time, page) for security and operations.
        </p>
        {earlyAccessConfigured ? (
          <p>
            If you join the early-access list, your email address and optional platform choice are sent to our sign-up service, used only to
            tell you about early access and releases, and deleted on request.
          </p>
        ) : (
          <p>The early-access list is not open, so this site collects no email addresses.</p>
        )}
        {site.contactEmail && (
          <p>
            Questions: <a href={`mailto:${site.contactEmail}`}>{site.contactEmail}</a>.
          </p>
        )}
        <p>
          Last updated <time dateTime="2026-10-22">22 October 2026</time>.
        </p>
      </Prose>
    </ContentPage>
  );
}
