import { expect, test } from "@playwright/test";
import { humanizeEta } from "@runmarq/contracts/format";
import { readFileSync } from "node:fs";
import { ROUTES } from "../lib/routes";

interface DemoFrame {
  label: string;
  change: string | null;
  run: { forecast: { remainingP10Seconds: number | null; remainingP50Seconds: number | null; remainingP90Seconds: number | null } | null };
}

const DEMO = JSON.parse(readFileSync(new URL("../content/demo-run.json", import.meta.url), "utf8")) as { frames: DemoFrame[] };

test.describe("routes", () => {
  for (const route of ROUTES) {
    test(`${route} renders with one h1 and no horizontal overflow`, async ({ page }) => {
      const res = await page.goto(route);
      expect(res?.status()).toBe(200);
      await expect(page.locator("h1")).toHaveCount(1);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow).toBeLessThanOrEqual(1);
    });
  }

  test("unknown pages return 404", async ({ page }) => {
    const res = await page.goto("/no-such-page");
    expect(res?.status()).toBe(404);
  });
});

test("no broken internal links", async ({ page, request }) => {
  const seen = new Set<string>();
  for (const route of ROUTES) {
    await page.goto(route);
    const hrefs = await page.$$eval("a[href]", (as) => as.map((a) => a.getAttribute("href") ?? ""));
    for (const h of hrefs) {
      if (!h.startsWith("/") || h.startsWith("//")) continue;
      seen.add(h.split("#")[0]!);
    }
  }
  for (const href of seen) {
    const res = await request.get(href);
    expect(res.status(), href).toBe(200);
  }
});

test("pre-release download page offers no dead download buttons", async ({ page }) => {
  await page.goto("/download");
  await expect(page.getByText("has not had a public release yet")).toBeVisible();
  await expect(page.locator("main").getByRole("link", { name: "Download", exact: true })).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Build it from source" })).toBeVisible();
});

test("early access endpoint refuses when no backend is configured", async ({ request }) => {
  const res = await request.post("/api/early-access", { data: { email: "a@b.co", consent: true } });
  expect(res.status()).toBe(503);
  expect((await res.json()).status).toBe("not_configured");
});

test("the demonstration responds to the visitor", async ({ page }) => {
  await page.goto("/");
  const demo = page.locator("#product");
  await expect(demo.getByText("Demo", { exact: true })).toBeVisible();
  const first = DEMO.frames[0]!.run.forecast!;
  await expect(demo.getByText(humanizeEta(first.remainingP10Seconds!, first.remainingP50Seconds!, first.remainingP90Seconds!).likely).first()).toBeVisible();
  const retry = DEMO.frames.find((f) => f.label === "Tests failed again")!;
  await demo.getByRole("button", { name: /Tests failed again/ }).click();
  await expect(demo.getByText(retry.change!).first()).toBeVisible();
  await demo.getByRole("button", { name: /Approval requested/ }).click();
  await expect(demo.getByText("Paused")).toBeVisible();
  await demo.getByRole("button", { name: /Run finished/ }).click();
  await expect(demo.getByText("Run finished").first()).toBeVisible();
});

test("integration tabs show each agent's manifest", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: /Cursor/ }).click();
  const panel = page.getByRole("tabpanel");
  await expect(panel.getByText(/Tool start and approval events are not collected/)).toBeVisible();
  await panel.getByRole("link", { name: "Cursor setup" }).click();
  await expect(page).toHaveURL(/\/docs\/integrations\/cursor$/);
});

test("docs search finds pages", async ({ page }) => {
  await page.goto("/docs");
  await page.getByRole("searchbox", { name: "Search documentation" }).fill("coverage");
  await expect(page.getByRole("link", { name: /How estimates work/ }).first()).toBeVisible();
});

test("keyboard users can skip to content and reach the primary action", async ({ page, isMobile }) => {
  test.skip(isMobile, "keyboard path is desktop");
  await page.goto("/");
  const action = (await page.locator("header .btn-primary").textContent())!.trim();
  await page.keyboard.press("Tab");
  await expect(page.getByRole("link", { name: "Skip to content" })).toBeFocused();
  for (let i = 0; i < 7; i++) await page.keyboard.press("Tab");
  const focused = await page.evaluate(() => document.activeElement?.textContent ?? "");
  expect(focused).toContain(action);
});

test("llms.txt indexes pages and command anchors that exist", async ({ page, request }) => {
  const res = await request.get("/llms.txt");
  expect(res.status()).toBe(200);
  expect(res.headers()["content-type"]).toContain("text/plain");
  const text = await res.text();
  expect(text.startsWith("# runmarq\n")).toBe(true);
  const links = [...text.matchAll(/\]\((https?:\/\/[^)]+)\)/g)].map((m) => new URL(m[1]!));
  expect(links.length).toBeGreaterThan(20);
  for (const path of new Set(links.map((u) => u.pathname))) {
    expect((await request.get(path)).status(), path).toBe(200);
  }
  await page.goto("/docs/command-line");
  for (const u of links.filter((l) => l.pathname === "/docs/command-line" && l.hash)) {
    await expect(page.locator(u.hash), u.hash).toHaveCount(1);
  }
});

test("mobile navigation opens and closes", async ({ page, isMobile }) => {
  test.skip(!isMobile, "mobile only");
  await page.goto("/");
  await page.getByRole("button", { name: "Open menu" }).click();
  const menu = page.getByRole("navigation", { name: "Mobile" });
  await expect(menu).toBeVisible();
  await menu.getByRole("link", { name: "Documentation" }).click();
  await expect(page).toHaveURL(/\/docs$/);
  await expect(menu).toBeHidden();
});
