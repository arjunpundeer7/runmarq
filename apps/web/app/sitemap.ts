import type { MetadataRoute } from "next";
import { ROUTES } from "@/lib/routes";
import { site } from "@/lib/site";

export const dynamic = "force-static";

/** Every public page, dated by the build that produced it. */
export default function sitemap(): MetadataRoute.Sitemap {
  const lastModified = new Date();
  return ROUTES.map((r) => ({
    url: new URL(r, site.url).toString(),
    lastModified,
    priority: r === "/" ? 1 : r.startsWith("/docs") ? 0.6 : 0.8,
  }));
}
