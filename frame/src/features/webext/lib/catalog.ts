import * as m from "$shared/i18n/messages";

/** Extensions verified to work well in Zephium, in daily use and in the
 *  real-extension suite. "Get" opens their Chrome Web Store page. */
export type CatalogEntry = {
  id: string;
  name: string;
  blurb: () => string;
};

export const catalog: CatalogEntry[] = [
  {
    id: "aeblfdkhhhdcdjpifhhbdiojplfjncoa",
    name: "1Password",
    blurb: m.webext_catalog_1password,
  },
  {
    id: "nngceckbapebfimnlniiiahkandclblb",
    name: "Bitwarden",
    blurb: m.webext_catalog_bitwarden,
  },
  {
    id: "kbfnbcaeplbcioakkpcpgfkobkghlhen",
    name: "Grammarly",
    blurb: m.webext_catalog_grammarly,
  },
  {
    id: "eimadpbcbfnmbkopoojfekhnkhdbieeh",
    name: "Dark Reader",
    blurb: m.webext_catalog_dark_reader,
  },
  {
    id: "dbepggeogbaibhgnhhndojpepiihcmeb",
    name: "Vimium",
    blurb: m.webext_catalog_vimium,
  },
  {
    id: "mnjggcdmjocbbbhaepdhchncahnbgone",
    name: "SponsorBlock",
    blurb: m.webext_catalog_sponsorblock,
  },
  {
    id: "gebbhagfogifgggkldgodflihgfeippi",
    name: "Return YouTube Dislike",
    blurb: m.webext_catalog_ryd,
  },
  {
    id: "cofdbpoegempjloogbagkncekinflcnj",
    name: "DeepL",
    blurb: m.webext_catalog_deepl,
  },
  {
    id: "ldgfbffkinooeloadekpmfoklnobpien",
    name: "Raindrop.io",
    blurb: m.webext_catalog_raindrop,
  },
  {
    id: "knheggckgoiihginacbkhaalnibhilkk",
    name: "Notion Web Clipper",
    blurb: m.webext_catalog_notion,
  },
  {
    id: "nffaoalbilbmmfgbnbgppjihopabppdk",
    name: "Video Speed Controller",
    blurb: m.webext_catalog_video_speed,
  },
  {
    id: "gppongmhjkpfnbhagpmjfkannfbllamg",
    name: "Wappalyzer",
    blurb: m.webext_catalog_wappalyzer,
  },
];

export const storeListing = (id: string) => `https://chromewebstore.google.com/detail/${id}`;
