import type { MetadataRoute } from "next";
import { ROUTES } from "@/lib/routes";
import { site } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  return ROUTES.map((r) => ({ url: new URL(r, site.url).toString() }));
}
