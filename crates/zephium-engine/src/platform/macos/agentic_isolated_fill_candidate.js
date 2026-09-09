// Release-excluded insertion into the fixed runtime's private lexical scope.
// No new runtime entry point or wire field; never installed by shipping builds.
const commandInsertText = Document.prototype.execCommand;
const commandCreateRange = Document.prototype.createRange;
const commandGetSelection = Document.prototype.getSelection;
const commandSelectContents = Range.prototype.selectNodeContents;
const commandRangeBounds = typeof AbstractRange === "function" ? AbstractRange.prototype : Range.prototype;
const commandRangeStart = getter(commandRangeBounds, "startContainer");
const commandRangeEnd = getter(commandRangeBounds, "endContainer");
const commandStartOffset = getter(commandRangeBounds, "startOffset");
const commandEndOffset = getter(commandRangeBounds, "endOffset");
const commandSelectionCount = getter(Selection.prototype, "rangeCount");
const commandGetRange = Selection.prototype.getRangeAt;
const commandRemoveRanges = Selection.prototype.removeAllRanges;
const commandAddRange = Selection.prototype.addRange;
const commandFocus = HTMLElement.prototype.focus;
const commandBlur = HTMLElement.prototype.blur;
let commandEntered = false;
let commandPreparationBarrier = null;
const commandPreparationOnly = false;
// Closed content-free diagnostics for this release-excluded candidate only.
// The native probe normalizes these to the original unsupported terminal.
let commandPreflightFailure = "witness";
function commandRefuse(reason) { commandPreflightFailure = reason; return null; }

function commandPlainText(node) {
  if (nodeType(node) === 3) {
    const text = read(characterDataGetter, node);
    return typeof text === "string" && text.length <= MAX_ACTION_TEXT_BYTES &&
      utf8Length(text, MAX_ACTION_TEXT_BYTES + 1) <= MAX_ACTION_TEXT_BYTES ? text : null;
  }
  if (nodeType(node) !== 1) return null;
  const children = read(nodeChildNodesGetter, node);
  if (listLength(children) > 128) return null;
  let value = "";
  for (let index = 0; index < listLength(children); index += 1) {
    const child = listItem(children, index);
    if (nodeType(child) !== 3) return null;
    const text = commandPlainText(child);
    if (text === null || value.length + text.length > MAX_ACTION_TEXT_BYTES) return null;
    value += text;
    if (utf8Length(value, MAX_ACTION_TEXT_BYTES + 1) > MAX_ACTION_TEXT_BYTES) return null;
  }
  return value;
}

function commandEditorWitness(target) {
  const ancestors = captureEditingContext(target);
  if (ancestors === null) return commandRefuse("ancestor_context");
  // The logical field is one plain leaf and its immediate container. WebKit's
  // effective editing host may be farther up an inherited editable spine.
  // Retain the exact bounded ancestor chain; never select its contents or scan
  // the surrounding rich document. These are locators/postconditions, not
  // confinement against same-origin page code.
  const declarations = [];
  for (const entry of ancestors) {
    const declaration = attribute(entry.node, "contenteditable", 16);
    if (declaration !== null && !["", "true", "false", "plaintext-only"].includes(lower(declaration)))
      return commandRefuse("ancestor_declaration");
    declarations.push(declaration === null ? null : lower(declaration));
  }
  const nested = ancestors.length > 0 && ancestors[0].editable;
  const root = nested ? ancestors[0].node : target;
  const siblings = [];
  if (nested) {
    const children = read(nodeChildNodesGetter, root);
    if (listLength(children) > 32) return commandRefuse("sibling_count");
    for (let index = 0; index < listLength(children); index += 1) {
      const node = listItem(children, index);
      if (node === target) continue;
      if (nodeType(node) !== 1) return commandRefuse("sibling_node_kind");
      if (attribute(node, "contenteditable", 16) !== "false") return commandRefuse("sibling_editability");
      // A protected div is admitted only under the same explicit noneditable,
      // public, bounded text-only contract. Its block layout grants no editing
      // operation; selection remains confined to the separate plain leaf.
      if (!["b", "strong", "i", "em", "span", "div"].includes(tagName(node))) {
        // Fixed diagnostic categories only; never emit a page-defined tag.
        switch (tagName(node)) {
          case "br": return commandRefuse("sibling_tag_br");
          case "wbr": return commandRefuse("sibling_tag_wbr");
          case "p": return commandRefuse("sibling_tag_p");
          default: return commandRefuse("sibling_tag_other");
        }
      }
      const text = commandPlainText(node);
      if (text === null) return commandRefuse("sibling_text");
      if (sensitivityFor(node) !== "public") return commandRefuse("sibling_sensitivity");
      siblings.push({ node, text, tag: tagName(node) });
    }
  }
  return { root, target, ancestors, declarations, siblings, nested };
}

function commandSpineMatches(witness) {
  return witness.ancestors.every((entry, index) => {
    const declaration = attribute(entry.node, "contenteditable", 16);
    return (declaration === null ? null : lower(declaration)) === witness.declarations[index];
  });
}

function commandEditorValue(witness) {
  const { root, target, ancestors, siblings, nested } = witness;
  if (read(nodeConnectedGetter, root) !== true ||
      read(nodeOwnerDocumentGetter, root) !== document ||
      apply(nodeGetRoot, root, []) !== document) return commandRefuse("root_identity");
  if (read(editableGetter, root) !== true) return commandRefuse("root_editability");
  if (disabledState(root, false) || has(root, "readonly") ||
      lower(attribute(root, "aria-readonly", 16) || "") === "true") return commandRefuse("root_writability");
  if (sensitivityFor(root) !== "public") return commandRefuse("root_sensitivity");
  if (!styleIsVisible(root)) return commandRefuse("root_visibility");
  if (credentialField(root, { role: "textbox", tag: tagName(root) }, attribute(root, "aria-label", 512) || "")) return commandRefuse("root_credential");
  const current = captureEditingContext(root);
  const expected = nested ? ancestors.slice(1) : ancestors;
  if (current === null || current.length !== expected.length ||
      !expected.every((item, index) => item.node === current[index].node && item.editable === current[index].editable)) return commandRefuse("root_context");
  if (!commandSpineMatches(witness)) return commandRefuse("ancestor_declaration");
  if (!nested) {
    const text = commandPlainText(target);
    return text === null ? commandRefuse("target_structure") : text;
  }
  const children = read(nodeChildNodesGetter, root);
  if (listLength(children) > 33) return commandRefuse("child_count");
  let value = "", protectedCount = 0, editableCount = 0;
  for (let index = 0; index < listLength(children); index += 1) {
    const node = listItem(children, index);
    const protectedNode = siblings.find(item => item.node === node);
    if (protectedNode) {
      if (read(nodeOwnerDocumentGetter, node) !== document) return commandRefuse("protected_identity");
      if (attribute(node, "contenteditable", 16) !== "false") return commandRefuse("protected_editability");
      if (tagName(node) !== protectedNode.tag) return commandRefuse("protected_tag");
      const protectedText = commandPlainText(node);
      if (protectedText === null) return commandRefuse("protected_structure");
      if (protectedText !== protectedNode.text) return commandRefuse("protected_text");
      if (sensitivityFor(node) !== "public") return commandRefuse("protected_sensitivity");
      protectedCount += 1;
      continue;
    }
    if (++editableCount > 1) return commandRefuse("editable_child_count");
    if (nodeType(node) === 1) {
      const descriptor = classify(node);
      if (descriptor === null || fillControlKind(descriptor, "") !== 3 ||
          disabledState(node, false) || has(node, "readonly") ||
          lower(attribute(node, "aria-readonly", 16) || "") === "true" ||
          credentialField(node, descriptor, attribute(node, "aria-label", 512) || "") ||
          sensitivityFor(node) !== "public") return commandRefuse("target_control");
    }
    const text = commandPlainText(node);
    if (text === null) return commandRefuse("target_structure");
    value += text;
  }
  return protectedCount === siblings.length ? value : commandRefuse("protected_count");
}

function runFixedFill(target, descriptor, request) {
  if (fillControlKind(descriptor, request.z) !== 3)
    return commandPreparationOnly ? "unsupported_interaction" : runSyntheticFill(target, descriptor, request);
  // The proof consumes at most one command opportunity for the entire owned
  // document, even after a new observation, cancellation, exception or retry.
  if (commandEntered) return "applied_unverified";
  const revalidate = (prepared = false) => {
    if (resolveKeyAtGeneration(request.t, request.g) !== target ||
        read(nodeConnectedGetter, target) !== true ||
        read(nodeOwnerDocumentGetter, target) !== document ||
        apply(nodeGetRoot, target, []) !== document) return false;
    const current = classify(target);
    const actual = runtimeDescriptor(target, request.g);
    const expected = prepared ? { ...request.f, s: request.f.s & ~64 } : request.f;
    if (prepared && actual !== null) actual.s &= ~64;
    return current !== null && fillControlKind(current, request.z) === 3 &&
      !disabledState(target, false) && !has(target, "readonly") &&
      lower(attribute(target, "aria-readonly", 16) || "") !== "true" &&
      !credentialField(target, current, attribute(target, "aria-label", 512) || "") &&
      editingContextMatches(target, request, current.editingContext) &&
      descriptorMatches(expected, actual);
  };
  let witness, selection, priorRange, priorFocus, range;
  let preflightStage = "primitives";
  try {
    if (!revalidate()) return "target_changed";
    if (typeof commandInsertText !== "function" || typeof commandFocus !== "function" ||
        !validActionText(request.z) || request.z.includes("\n") || request.z.includes("\t")) return "unsupported_interaction_primitives_or_text";
    preflightStage = "witness";
    witness = commandEditorWitness(target);
    if (witness === null) return "unsupported_interaction_" + commandPreflightFailure;
    preflightStage = "value";
    commandPreflightFailure = "logical_value";
    if (commandEditorValue(witness) !== commandPlainText(target)) return "unsupported_interaction_" + commandPreflightFailure;
    if (commandEditorValue(witness) === request.z) return "unsupported_interaction_value_unchanged";
    preflightStage = "selection";
    selection = apply(commandGetSelection, document, []);
    const count = read(commandSelectionCount, selection);
    if (count > 1) return "unsupported_interaction_selection_count";
    priorRange = count === 1 ? apply(commandGetRange, selection, [0]) : null;
    priorFocus = read(documentActiveGetter, document);
    preflightStage = "range";
    range = apply(commandCreateRange, document, []);
    apply(commandSelectContents, range, [target]);
  } catch (_) { return "unsupported_interaction_exception_" + preflightStage; }
  // Focus can synchronously run page handlers too. All later paths are uncertain.
  commandEntered = true;
  const preparedCurrent = () => {
    if (!revalidate(true) || !commandSpineMatches(witness) || read(commandSelectionCount, selection) !== 1) return false;
    const actual = apply(commandGetRange, selection, [0]);
    const children = read(nodeChildNodesGetter, target);
    const childCount = listLength(children);
    const first = childCount ? listItem(children, 0) : null;
    const last = childCount ? listItem(children, childCount - 1) : null;
    const rootChildren = read(nodeChildNodesGetter, witness.root);
    let targetIndex = -1;
    for (let index = 0; witness.nested && index < listLength(rootChildren); index += 1) {
      if (listItem(rootChildren, index) === target) targetIndex = index;
    }
    const startsAtTarget = (read(commandStartOffset, actual) === 0 &&
      (read(commandRangeStart, actual) === target || read(commandRangeStart, actual) === first)) ||
      (targetIndex >= 0 && read(commandRangeStart, actual) === witness.root && read(commandStartOffset, actual) === targetIndex);
    const endsAtTarget = (read(commandRangeEnd, actual) === target && read(commandEndOffset, actual) === childCount) ||
      (last !== null && read(commandRangeEnd, actual) === last &&
        read(commandEndOffset, actual) === read(characterDataGetter, last).length) ||
      (targetIndex >= 0 && read(commandRangeEnd, actual) === witness.root && read(commandEndOffset, actual) === targetIndex + 1);
    if (!revalidate(true) || !commandSpineMatches(witness) || !startsAtTarget || !endsAtTarget) return false;
    const rect = elementRect(target), viewport = boundedViewport();
    return styleIsVisible(target) && rect !== null && geometryCompatible(request.e, rect) &&
      viewport !== null && actionPoint(target, rect, viewport) !== null;
  };
  try {
    apply(commandFocus, target, [{ preventScroll: true }]);
    if (!revalidate(true) || !commandSpineMatches(witness)) return "applied_unverified_beforeinput_revalidation";
    apply(commandRemoveRanges, selection, []);
    apply(commandAddRange, selection, [range]);
    if (!preparedCurrent()) return "applied_unverified_beforeinput_revalidation";
  } catch (_) { return "applied_unverified_beforeinput_revalidation"; }
  const insertPrepared = () => {
    let commandBehavior = "";
    try {
      // Preparation never authorizes a later changed target/selection. The host
      // separately owns the delayed release's original authority/deadline.
      if (!preparedCurrent()) return "applied_unverified_beforeinput_revalidation";
      // The consumed opportunity includes focus/range effects; this terminal
      // asserts only that the insertion command was never entered.
      if (commandPreparationOnly) return "applied_unverified_preparation_only";
      // Diagnostic-only return/observation categories; neither admits success.
      const returned = apply(commandInsertText, document, ["insertText", false, request.z]);
      let immediate = "exception";
      try {
        const value = commandEditorValue(witness);
        immediate = value === request.z ? "match" : value === null ? "guarded" : "mismatch";
      } catch (_) { /* Preserve the original command/result path. */ }
      commandBehavior = "_command_" + (returned === true ? "true" : returned === false ? "false" : "other") + "_immediate_" + immediate;
    } catch (_) { return "applied_unverified_mutation"; }
    // This fresh private view has no human selection to restore. Its owner
    // retains uncertain action debt until terminal/teardown. The microtask
    // observes reconciliation, but cannot mint exact-ref proof or persistence.
    return apply(promiseThen, apply(promiseResolve, nativePromise, []), [() => {
      try {
        commandPreflightFailure = "logical_value";
        const value = commandEditorValue(witness);
        return (value === request.z ? "applied_unverified_logical_editor"
          : "applied_unverified_postcondition_" + (value === null ? commandPreflightFailure : "value_mismatch")) + commandBehavior;
      } catch (_) { return "applied_unverified_postcondition_reconciliation_exception" + commandBehavior; }
    }]);
  };
  if (commandPreparationBarrier === null) return insertPrepared();
  try {
    return apply(promiseThen, commandPreparationBarrier(), [
      permit => permit === "ZEPHIUM_PREPARED_FILL_CONTINUE_V1" ? insertPrepared() : "applied_unverified_beforeinput_revalidation",
      () => "applied_unverified_beforeinput_revalidation"
    ]);
  } catch (_) { return "applied_unverified_beforeinput_revalidation"; }
}
