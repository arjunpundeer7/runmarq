import { AGENT_DOCS } from "@/content/integrations";

/** Every public page, used by the sitemap and the link-check tests. */
export const ROUTES = [
  "/",
  "/features",
  "/how-it-works",
  "/download",
  "/docs",
  "/docs/getting-started",
  ...AGENT_DOCS.map((d) => `/docs/integrations/${d.slug}`),
  "/docs/eta-explained",
  "/docs/command-line",
  "/docs/troubleshooting",
  "/privacy",
  "/terms",
  "/changelog",
  "/contact",
];
