import { commands } from "$shared/ipc/bindings";

const icons = new Map<string, Promise<string | null>>();

/** A catalog icon as a data URL. The browser keeps each on disk once fetched;
 *  this keeps them for the session, and forgets a failed fetch so the next
 *  visit tries again. */
export function catalogIcon(token: string): Promise<string | null> {
  let icon = icons.get(token);
  if (icon === undefined) {
    icon = commands.webExtensionCatalogIcon(token).then(
      (result) => (result.status === "ok" ? result.data : null),
      () => null,
    );
    icons.set(token, icon);
    void icon.then((url) => {
      if (url === null) icons.delete(token);
    });
  }
  return icon;
}
