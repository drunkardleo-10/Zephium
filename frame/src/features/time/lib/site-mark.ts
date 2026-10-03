import type { IconRef } from "$shared/ipc/bindings";
import { favicons } from "$domain/favicons";
import { focus } from "$domain/time";

/** A site's icon from whatever is held: the reference given with it, the
 *  reference focus listed for it, or a raster for the site or its `www.`
 *  host, which is where most sites actually serve their pages. */
export function siteMark(site: string, ref?: IconRef | null) {
  const listed = ref ?? focus.shut().find((entry) => entry.site === site)?.icon ?? null;
  return (
    favicons.mark(listed, `https://${site}/`) ?? favicons.forPage(`https://www.${site}/`) ?? null
  );
}
