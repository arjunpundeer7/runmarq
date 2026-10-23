import type { Metadata } from "next";
import { docMetadata } from "@/content/docs";
import { CodeBlock } from "@/components/CodeBlock";
import { PageTitle, Prose } from "@/components/Prose";
import { COMMANDS, commandId, WAIT_EXIT_CODES } from "@/content/commands";

export const metadata: Metadata = docMetadata("/docs/command-line");

export default function CommandLine() {
  return (
    <>
      <PageTitle title="Command line" />
      <Prose>
        <p>
          Status, doctor, history, eval, wait, statusline and the integration commands print JSON with <code>--json</code>. Set{" "}
          <code>RUNMARQ_HOME</code> to use a different data folder.
        </p>
        {COMMANDS.map((g) => (
          <section key={g.group}>
            <h2>{g.group}</h2>
            {g.items.map((c) => (
              <div key={c.usage}>
                <h3 id={commandId(c.usage)} className="scroll-mt-28">
                  <code>{c.usage}</code>
                </h3>
                <p>{c.what}</p>
                {c.options && (
                  <table>
                    <tbody>
                      {c.options.map(([flag, text]) => (
                        <tr key={flag}>
                          <td>
                            <code>{flag}</code>
                          </td>
                          <td>{text}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                )}
              </div>
            ))}
          </section>
        ))}

        <h2 id="wait-exit-codes">
          Exit codes of <code>runmarq wait</code>
        </h2>
        <table>
          <tbody>
            {WAIT_EXIT_CODES.map(([code, text]) => (
              <tr key={code}>
                <td>
                  <code>{code}</code>
                </td>
                <td>{text}</td>
              </tr>
            ))}
          </tbody>
        </table>

        <h2 id="status-line">Status line in Claude Code</h2>
        <p>
          Add this to <code>~/.claude/settings.json</code>, or to a project&apos;s <code>.claude/settings.json</code>, to see your agents
          under Claude Code&apos;s prompt:
        </p>
        <CodeBlock>{`"statusLine": { "type": "command", "command": "runmarq statusline" }`}</CodeBlock>
        <p>In tmux:</p>
        <CodeBlock>{`set -g status-right '#(runmarq statusline)'
set -g status-interval 5`}</CodeBlock>
      </Prose>
    </>
  );
}
