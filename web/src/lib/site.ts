import type { SiteConfig } from "../api/gen/SiteConfig";
import type { SiteView } from "../api/gen/SiteView";
import { ApiError, api } from "./api";

/**
 * Saves the site's next revision: `edit` applied to the latest document, read just before, so
 * that the save builds on what another tab may have saved. Once another save wins the race
 * (409), it reads and edits again. A refused document throws its 422 with the issues.
 */
export async function saveSite(
  edit: (config: SiteConfig) => SiteConfig,
  message: string,
  secrets?: Record<string, string | null>,
): Promise<SiteView> {
  const once = async () => {
    const latest = await api.site.get();
    return api.site.save({
      base: latest.revision.id,
      message,
      config: edit(latest.config),
      ...(secrets && { secrets }),
    });
  };
  return once().catch((err) => {
    if (err instanceof ApiError && err.status === 409) return once();
    throw err;
  });
}
