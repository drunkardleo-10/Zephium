# Zephium interface system

The implementation follows `product-system.md`: Browse stands alone; Work
makes useful state visible. Geist, neutral surfaces, optical alignment, and
restrained motion give both environments one identity. The system lives in
`frame/src/shared/ui`, with semantic tokens and shared styles in `frame/src/styles`.

## Inspect the interface

Run `pnpm dev` and open Settings from the browser's profile menu. Settings is
part of the actual browser window, with navigation in the sidebar and its
content in the main area. Interface Studio and its separate entry points have
been removed. See [Settings interface](settings.md) for the current section
map, preview-state contract, and language-selection behavior.

## Tokens and composition

Use semantic colors, never raw palette utilities in product markup. The
surface ladder is canvas → surface → raised; transient chrome fills remain
transparent overlays on a real native material. Material chrome uses stronger
foreground tokens to remain legible over varying backgrounds. The original browser controls are the styling reference. Do not add CSS blur, backdrop
filters, decorative color gradients, or glow.

The rhythm is 8 px with 4 px subdivisions. Chrome controls are 28–36 px; general
buttons also have a 40 px large size. Icons are optically consistent at 16 px in
control clusters. Radius tokens span 5, 7, 9, 12, and 16 px. The native content
radius and startup canvas colors remain shared contracts, not freely adjustable
theme knobs. Body type is 13 px, labels 12 px, captions 11 px; page titles and
specimen display sizes are intentionally larger.

Buttons and inputs use native HTML semantics. Form submit buttons opt into
`type="submit"`. Inputs have generated stable IDs and associated help/error
text. Native selects and radio groups retain platform keyboard behavior. No
new overlay behavior or cross-WebView portals are introduced.

| Component | Contract |
| --- | --- |
| Button | `variant`: primary/secondary/ghost/danger; `size`: compact/regular/large; `pending` disables activation and exposes busy state; native button attributes pass through |
| IconButton | Existing icon/label/menu contract plus optional pending state; keep labels descriptive |
| Field / TextArea | Required label, bindable string value, optional hint/error; native input attributes pass through |
| SearchField | Required accessible label, bindable query, trimmed `onsubmit(value)`; does not navigate by itself |
| Toggle | Checkbox or switch, label/description, bindable checked value, disabled, `onchange(checked)` |
| Select / ChoiceGroup | Typed value/label options, bindable value; ChoiceGroup optionally uses segmented presentation |
| Badge | Neutral/accent/success/warning/danger with an optional nonsemantic status dot; always include status text |
| Surface | Canvas/surface/raised; layout belongs to the caller |
| ListRow | Selection button with label, description, leading/trailing snippets and callback; do not nest interactive controls |
| Progress | Label, optional value, max; native progress semantics |
| EmptyState | Title, description, optional icon/action snippets |
| Separator / KeyHint | Semantic divider and platform-specific shortcut presentation |

All these components are presentation-only. They do not import domain stores,
call IPC, persist settings, fetch resources, or claim native success. For live
operations, the parent holds authoritative state and waits for settlement. A
checkbox changing in a development fixture is not a durable domain commit.

## Browse and Work

Live Browse uses the shared icon control, common action styles in consent
surfaces, clarified active-tab treatment, stronger address/search focus,
consistent navigation icon sizing, and the quieter new-tab typography. Existing
native menus, extension behavior, permission focus traps, sidebar structure,
compact-mode address sentinel, and tab presentation barriers remain intact.
The authoritative title sentinel still contains only the exact tab title.

The unused Work fixture kit has been removed during the frontend foundation
migration. Future Work and entity presentations will use the same shared primitives,
with state supplied through the real Rust contracts. No production Work workflow
is implemented by this UI foundation.

## Localization and motion

New-tab, navigation, address and tab-action strings use compiled Paraglide
messages from `frame/messages/en.json`. The pinned message-format plugin loads
from local dependencies; compilation needs no remote plugin download. `pnpm -C
frame i18n` regenerates ignored output, and typecheck/build run it automatically.
Components receive localized strings from their owners. Existing untouched
feature copy remains an incremental localization migration, not a claim of a
fully translated product. Text expansion and locale stress belong in development tests, never in persisted fake locale settings.

The existing Browse treatment uses 120 ms feedback and 180 ms transitions, at
most 220 ms. Switches and segmented selections use the timings documented below. Animate
transform and opacity, with narrowly scoped color feedback. No shadow, blur,
layout, startup, or native geometry animation. Native startup and tab-presentation barriers remain independent of decorative motion. There is
no recurring animation or polling in the component kit.

## Native material

`UiInfo.material` is a generated enum: none, vibrancy, liquid_glass, acrylic, mica.
It represents the effect installed on the requesting window. `material.*`
messages on the existing scoped UI-command transport update it at runtime;
a later startup query cannot overwrite an already received update.

macOS uses window-vibrancy 0.8.0: public Regular Liquid Glass on supported
macOS 26+, Sidebar/HudWindow vibrancy as fallback, and opaque CSS surfaces if
neither installation succeeds. Reduce Transparency skips effects entirely.
An AppKit accessibility observer reapplies the material on settings changes;
observers and per-window records are removed on destruction. All NSView work
stays on the main thread.

The material is installed below the existing Tao content-view children, with
no `content_view()` reparenting. WKWebView parent bounds, coordinate translation,
page sibling order and the chrome rectangle remain unchanged. Native theme
changes update AppKit, rather than merely retinting HTML. The launcher retains
its existing 16 px radius and the main window its existing 12 px radius.
Windows keeps its Acrylic policy with per-window reporting; Linux stays solid.
The enum includes Mica for truthful platform reporting, not a new Windows policy.

## Original browser controls are the visual baseline

The rejected material-study stylesheet has been removed. Live
Browse uses the original semantic palette; there is no independent gallery
palette or control skin.

The original `IconButton` styling and address/new-tab input wrappers are the
reference. Buttons have quiet resting states, existing rounded-rectangle
radii, and translucent hover/press feedback. Text Button extends this language
with padding and semantic primary/secondary variants. Inputs use the original
34 px address-field geometry, fill, and faint inset highlight. SearchField also
provides a `page` size matching the original 48 px new-tab search. Keyboard
focus, labels, validation, and disabled behavior remain explicit.

SettingsGroup/SettingsRow supply grouped composition. Switches keep their bright
thumb and native-like proportions. Segmented selection uses the original
selected-surface fill and 240 ms transform travel; no palette override or
measurement loop is needed. Reduced motion and RTL remain supported.
