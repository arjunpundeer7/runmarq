// Product naming lives in brand.json so it can change in one place.
// The Rust crates read the same file at compile time.
import data from "../brand.json";

export interface Brand {
  productName: string;
  cliName: string;
  dataDirName: string;
  bundleIdentifier: string;
  tagline: string;
  description: string;
  /** Public repository URL. `null` hides every GitHub link. */
  githubUrl: string | null;
  /** Public contact address. `null` shows the no-contact-yet state. */
  contactEmail: string | null;
  /** Canonical site URL for metadata. */
  siteUrl: string | null;
}

export const brand = data as Brand;
