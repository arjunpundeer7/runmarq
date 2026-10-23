// Site-wide facts, all derived from maintained sources: the brand file and
// the release manifest. Nothing here is a claim that the code cannot back up.
import { brand } from "@runmarq/config";
import { LATEST_RELEASE } from "@runmarq/contracts";
import type { Metadata } from "next";

/** A deploy-time setting, read when the site is built; blank counts as unset. */
function setting(name: string): string | null {
  return process.env[name]?.trim() || null;
}

/** The public address: SITE_URL, else the brand file, else Vercel's production domain. */
function siteUrl(): string {
  const vercel = setting("VERCEL_PROJECT_PRODUCTION_URL");
  return setting("SITE_URL") ?? brand.siteUrl ?? (vercel ? `https://${vercel}` : "http://localhost:3000");
}

export const site = {
  name: brand.productName,
  cli: brand.cliName,
  description: brand.description,
  githubUrl: setting("GITHUB_URL") ?? brand.githubUrl,
  contactEmail: setting("CONTACT_EMAIL") ?? brand.contactEmail,
  url: siteUrl().replace(/\/$/, ""),
};

/** The shared link-preview image, `app/opengraph-image.png`. */
const PREVIEW_IMAGE = { url: "/opengraph-image.png", width: 1200, height: 630, alt: `${brand.productName}: know when your coding agent will finish` };

/**
 * A page's title, description, canonical address and link preview. Pages
 * replace the layout's Open Graph fields, image included, so the shared
 * ones are repeated here.
 */
export function pageMetadata(path: string, title: string | null, description: string): Metadata {
  return {
    ...(title ? { title } : {}),
    description,
    alternates: { canonical: path },
    openGraph: {
      type: "website",
      siteName: site.name,
      title: title ?? site.name,
      description,
      url: path,
      images: [PREVIEW_IMAGE],
    },
  };
}

/** True only when the release manifest lists real, checksummed artifacts. */
export const released = LATEST_RELEASE != null;

/** Early access sign-up works only when a backend is configured. */
export const earlyAccessConfigured = Boolean(process.env.EARLY_ACCESS_WEBHOOK_URL);

/** The one action every page offers: whatever actually gets runmarq onto a computer today. */
export const primaryCta = released
  ? { label: `Download ${site.name}`, href: "/download" }
  : earlyAccessConfigured
    ? { label: "Get early access", href: "/download#early-access" }
    : { label: "Build from source", href: "/download#source" };

/** The repository's clone address, when the code is public. */
export const cloneUrl = site.githubUrl ? `${site.githubUrl.replace(/\/$/, "")}.git` : null;

/** Getting the code and building it, as the README shows. */
export const buildSteps = [
  ...(cloneUrl ? [`git clone ${cloneUrl}`, "cd runmarq"] : []),
  "pnpm install",
  "cargo build --release -p runmarq-cli",
  "pnpm build:desktop",
].join("\n");
