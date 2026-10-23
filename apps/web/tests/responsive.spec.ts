import { expect, test } from "@playwright/test";
import { ROUTES } from "../lib/routes";

test.describe("responsive layouts", () => {
  // Run each explicit viewport once rather than repeat the matrix per device project.
  test.beforeEach(({}, testInfo) => {
    test.skip(testInfo.project.name !== "desktop", "Explicit viewport matrix runs in the desktop project");
  });
  test.use({ reducedMotion: "reduce" });

  for (const width of [320, 390, 640, 768, 820, 1024, 1280, 1440, 1920, 2560]) {
    test(`public pages fit a ${width}px viewport`, async ({ page }) => {
      test.setTimeout(90_000);
      await page.setViewportSize({ width, height: 900 });
      for (const route of ROUTES) {
        const response = await page.goto(route);
        expect(response?.status(), route).toBe(200);
        await page.evaluate(() => document.fonts.ready);
        const overflow = await page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
        expect(overflow, `${route} at ${width}px`).toBeLessThanOrEqual(1);
      }
    });

    test(`home content stays inside its columns at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto("/");
      await page.evaluate(() => document.fonts.ready);

      const outside = await page.locator("main [data-ui]").evaluateAll((fragments) =>
        fragments.flatMap((fragment) => {
          const rect = fragment.getBoundingClientRect();
          const parent = fragment.parentElement!.getBoundingClientRect();
          return rect.left < parent.left - 1 || rect.right > parent.right + 1
            ? [fragment.textContent?.slice(0, 80)]
            : [];
        }),
      );
      expect(outside).toEqual([]);

      const headline = await page.locator("h1").evaluate((el) => ({
        height: el.getBoundingClientRect().height,
        lineHeight: parseFloat(getComputedStyle(el).lineHeight),
      }));
      expect(headline.height / headline.lineHeight).toBeLessThan(2.2);

      const toast = await page.locator("[data-ui]").filter({ hasText: "Claude Code finished" }).boundingBox();
      const text = await page.getByRole("heading", { name: "Work without checking." }).locator("..").boundingBox();
      expect(toast).not.toBeNull();
      expect(text).not.toBeNull();
      const overlaps = toast!.x < text!.x + text!.width && toast!.x + toast!.width > text!.x &&
        toast!.y < text!.y + text!.height && toast!.y + toast!.height > text!.y;
      expect(overlaps).toBe(false);
    });
  }

  test("tablet menu releases the scroll lock when resized to desktop", async ({ page }) => {
    await page.setViewportSize({ width: 820, height: 900 });
    await page.goto("/");
    await page.getByRole("button", { name: "Open menu" }).click();
    await expect(page.locator("body")).toHaveCSS("overflow", "hidden");
    await page.setViewportSize({ width: 1280, height: 900 });
    await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
    await expect(page.locator("#mobile-menu")).toHaveCount(0);
    await page.setViewportSize({ width: 820, height: 900 });
    await expect(page.getByRole("button", { name: "Open menu" })).toHaveAttribute("aria-expanded", "false");
  });

  test("landscape menu scrolls to its primary action", async ({ page }) => {
    await page.setViewportSize({ width: 667, height: 375 });
    await page.goto("/");
    await page.getByRole("button", { name: "Open menu" }).click();
    const menu = page.locator("#mobile-menu");
    const action = menu.locator(".btn-primary");
    const href = (await action.getAttribute("href"))!;
    await action.scrollIntoViewIfNeeded();
    await expect(action).toBeInViewport();
    await action.click();
    await expect(page).toHaveURL(new URL(href, page.url()).toString());
    await expect(page.locator("body")).not.toHaveCSS("overflow", "hidden");
  });

  test("mobile docs navigation is available before the article", async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto("/docs/getting-started");
    await page.getByText("Browse documentation", { exact: true }).click();
    await page.locator("main").getByRole("navigation", { name: "Documentation", exact: true })
      .getByRole("link", { name: "Troubleshooting" }).click();
    await expect(page).toHaveURL(/\/docs\/troubleshooting$/);
  });
});
