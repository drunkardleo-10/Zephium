#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::dbg_macro, clippy::print_stderr, clippy::print_stdout)]
#![cfg_attr(
    not(test),
    deny(clippy::panic, clippy::unreachable, clippy::unwrap_used)
)]

//! Private production semantic runtime registration for owned agent contexts.
//!
//! WebKit's public evaluation APIs confer user activation, so this adapter
//! never evaluates JavaScript. The immutable document-start program instead
//! holds one Promise on a content-world-scoped reply handler. Native code may
//! answer that pull only with [`SemanticRuntimeInvocation`]'s closed grammar.

use std::cell::RefCell;
use std::panic::AssertUnwindSafe;
use std::ptr::null_mut;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use block2::RcBlock;
use objc2::{define_class, msg_send, rc::Retained, runtime::AnyObject, runtime::NSObject};
use objc2::{DefinedClass as _, MainThreadOnly, Message as _};
use objc2_foundation::{MainThreadMarker, NSObjectProtocol, NSString, NSUTF8StringEncoding};
use objc2_web_kit::{
    WKContentWorld, WKScriptMessage, WKScriptMessageHandlerWithReply, WKUserContentController,
    WKUserScript, WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration,
};
use zephium_agentic::{
    SemanticActionAttemptId, SemanticActionRuntimeEvidence, SemanticActionRuntimeInvocation,
    SemanticActionRuntimeResultError, SemanticRuntimeInvocation, SemanticRuntimeResultError,
    SemanticSnapshot, MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES,
    MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS, SEMANTIC_RUNTIME_CHANNEL_ACK,
    SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED, SEMANTIC_RUNTIME_CHANNEL_NAME,
    SEMANTIC_RUNTIME_CHANNEL_PULL, SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX,
    SEMANTIC_RUNTIME_CHANNEL_STOP, SEMANTIC_RUNTIME_PROGRAM,
};

const SEMANTIC_RUNTIME_WORLD_NAME_PREFIX: &str = "zephium-semantic-runtime-v1-";
const SEMANTIC_RUNTIME_FIXED_ERROR: &str = "zephium semantic channel refused";
/// Immutable page-world compatibility shim installed only by the owned-agent
/// view constructor. Its attributes are untrusted transport hints: they can
/// never authorize success. The shim has no native bridge, selectors,
/// arbitrary script input, activation route, or cross-document authority.
/// Main-frame scope is provided and attested by native `forMainFrameOnly`.
const PAGE_WORLD_COMPATIBILITY_FILL_PROGRAM: &str = r#"(() => {
  'use strict';
  const READY = 'data-zephium-fill-relay-ready-v1';
  const COMMAND = 'data-zephium-fill-relay-command-v1';
  const TERMINAL = 'data-zephium-fill-relay-terminal-v1';
  const MAX_TEXT_BYTES = 4096;
  const MAX_COMMAND_BYTES = 16384;
  const MAX_ATTRIBUTE_BYTES = 512;
  const MAX_METADATA_ID_BYTES = 1024;
  const MAX_LABEL_NODES = 128;
  const MAX_LABELS = 4;
  const MAX_RECORDS = 64;
  const MAX_BOOTSTRAP_RECORDS = 16;
  const MAX_SAFE_INTEGER = 9007199254740991;
  const mainGlobal = globalThis;
  const mainDocument = document;
  const apply = Reflect.apply;
  const getOwnDescriptor = Object.getOwnPropertyDescriptor;
  const getPrototypeOf = Object.getPrototypeOf;
  const objectKeys = Object.keys;
  const jsonParse = JSON.parse;
  const numberIsSafeInteger = Number.isSafeInteger;
  const stringCharCodeAt = String.prototype.charCodeAt;
  const stringIncludes = String.prototype.includes;
  const stringSplit = String.prototype.split;
  const stringToLowerCase = String.prototype.toLowerCase;
  const getAttribute = Element.prototype.getAttribute;
  const setAttribute = Element.prototype.setAttribute;
  const removeAttribute = Element.prototype.removeAttribute;
  const addEventListener = EventTarget.prototype.addEventListener;
  const removeEventListener = EventTarget.prototype.removeEventListener;
  const dispatchEvent = EventTarget.prototype.dispatchEvent;
  const NativeMutationObserver = MutationObserver;
  const observerObserve = MutationObserver.prototype.observe;
  const observerDisconnect = MutationObserver.prototype.disconnect;
  const mutationRecordTargetGet = getOwnDescriptor(MutationRecord.prototype, 'target')?.get;
  const mutationRecordAttributeNameGet = getOwnDescriptor(MutationRecord.prototype, 'attributeName')?.get;
  const documentElementGet = getOwnDescriptor(Document.prototype, 'documentElement')?.get;
  const documentDefaultViewGet = getOwnDescriptor(Document.prototype, 'defaultView')?.get;
  const documentGetElementById = Document.prototype.getElementById;
  // Main-frame scope is native authority: WebKit installs this fixed script
  // with forMainFrameOnly=true and Rust attests its exact source/inventory.
  // Window.prototype does not own frameElement in this measured WebKit realm,
  // and a dynamic property read would be page-forgeable. Keep only the exact
  // Document.defaultView identity check here.
  const inputPrototype = HTMLInputElement.prototype;
  const inputValueDescriptor = getOwnDescriptor(inputPrototype, 'value');
  const inputTypeDescriptor = getOwnDescriptor(inputPrototype, 'type');
  const inputDisabledDescriptor = getOwnDescriptor(inputPrototype, 'disabled');
  const inputReadOnlyDescriptor = getOwnDescriptor(inputPrototype, 'readOnly');
  const inputValueSet = inputValueDescriptor && inputValueDescriptor.set;
  const inputTypeGet = inputTypeDescriptor && inputTypeDescriptor.get;
  const inputDisabledGet = inputDisabledDescriptor && inputDisabledDescriptor.get;
  const inputReadOnlyGet = inputReadOnlyDescriptor && inputReadOnlyDescriptor.get;
  const inputLabelsGet = getOwnDescriptor(inputPrototype, 'labels')?.get;
  const textareaPrototype = HTMLTextAreaElement.prototype;
  const textareaValueDescriptor = getOwnDescriptor(textareaPrototype, 'value');
  const textareaDisabledDescriptor = getOwnDescriptor(textareaPrototype, 'disabled');
  const textareaReadOnlyDescriptor = getOwnDescriptor(textareaPrototype, 'readOnly');
  const textareaValueSet = textareaValueDescriptor && textareaValueDescriptor.set;
  const textareaDisabledGet = textareaDisabledDescriptor && textareaDisabledDescriptor.get;
  const textareaReadOnlyGet = textareaReadOnlyDescriptor && textareaReadOnlyDescriptor.get;
  const textareaLabelsGet = getOwnDescriptor(textareaPrototype, 'labels')?.get;
  const nodeTypeGet = getOwnDescriptor(Node.prototype, 'nodeType')?.get;
  const nodeOwnerDocumentGet = getOwnDescriptor(Node.prototype, 'ownerDocument')?.get;
  const nodeChildNodesGet = getOwnDescriptor(Node.prototype, 'childNodes')?.get;
  const nodeConnectedDescriptor = getOwnDescriptor(Node.prototype, 'isConnected');
  const nodeRoot = Node.prototype.getRootNode;
  const nodeConnectedGet = nodeConnectedDescriptor && nodeConnectedDescriptor.get;
  const characterDataGet = getOwnDescriptor(CharacterData.prototype, 'data')?.get;
  const nodeListLengthGet = getOwnDescriptor(NodeList.prototype, 'length')?.get;
  const nodeListItem = NodeList.prototype.item;
  const NativeInputEvent = InputEvent;

  const read = (getter, receiver) => apply(getter, receiver, []);
  const attr = (target, name) => apply(getAttribute, target, [name]);
  const setAttr = (target, name, value) => apply(setAttribute, target, [name, value]);
  const removeAttr = (target, name) => apply(removeAttribute, target, [name]);
  const contains = (value, needle) => apply(stringIncludes, value, [needle]);
  const lowerText = (value) => apply(stringToLowerCase, value, []);
  const utf8Length = (value, ceiling) => {
    let bytes = 0;
    for (let index = 0; index < value.length; index += 1) {
      const first = apply(stringCharCodeAt, value, [index]);
      let point = first;
      if (first >= 0xd800 && first <= 0xdbff) {
        if (index + 1 >= value.length) return ceiling + 1;
        const second = apply(stringCharCodeAt, value, [index + 1]);
        if (second < 0xdc00 || second > 0xdfff) return ceiling + 1;
        point = 0x10000 + ((first - 0xd800) << 10) + second - 0xdc00;
        index += 1;
      } else if (first >= 0xdc00 && first <= 0xdfff) {
        return ceiling + 1;
      }
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
      if (bytes > ceiling) return bytes;
    }
    return bytes;
  };
  const forbiddenPoint = (point) =>
    point < 0x20 || (point >= 0x7f && point <= 0x9f) ||
    point === 0xad || point === 0x61c || point === 0x180e ||
    (point >= 0x200b && point <= 0x200f) ||
    (point >= 0x202a && point <= 0x202e) ||
    (point >= 0x2060 && point <= 0x2064) ||
    (point >= 0x2066 && point <= 0x206f) || point === 0xfeff ||
    (point >= 0xfff9 && point <= 0xfffb) || point === 0xe0001 ||
    (point >= 0xe0020 && point <= 0xe007f);
  const validText = (value, multiline) => {
    if (typeof value !== 'string' || utf8Length(value, MAX_TEXT_BYTES + 1) > MAX_TEXT_BYTES) {
      return false;
    }
    for (let index = 0; index < value.length; index += 1) {
      const first = apply(stringCharCodeAt, value, [index]);
      let point = first;
      if (first >= 0xd800 && first <= 0xdbff) {
        const second = apply(stringCharCodeAt, value, [index + 1]);
        point = 0x10000 + ((first - 0xd800) << 10) + second - 0xdc00;
        index += 1;
      }
      if (point === 0x0d || (!multiline && (point === 0x09 || point === 0x0a))) return false;
      if (point !== 0x09 && point !== 0x0a && forbiddenPoint(point)) return false;
    }
    return true;
  };
  const boundedMetadata = (raw, limit) => {
    if (typeof raw !== 'string' || utf8Length(raw, limit + 1) > limit) return null;
    for (let index = 0; index < raw.length; index += 1) {
      const first = apply(stringCharCodeAt, raw, [index]);
      let point = first;
      if (first >= 0xd800 && first <= 0xdbff) {
        if (index + 1 >= raw.length) return null;
        const second = apply(stringCharCodeAt, raw, [index + 1]);
        if (second < 0xdc00 || second > 0xdfff) return null;
        point = 0x10000 + ((first - 0xd800) << 10) + second - 0xdc00;
        index += 1;
      } else if (first >= 0xdc00 && first <= 0xdfff) {
        return null;
      }
      if (
        point === 0x0d ||
        (point !== 0x09 && point !== 0x0a && forbiddenPoint(point))
      ) return null;
    }
    return raw;
  };
  const listLength = (list) => {
    const length = read(nodeListLengthGet, list);
    return numberIsSafeInteger(length) && length >= 0 ? length : -1;
  };
  const listItem = (list, index) => apply(nodeListItem, list, [index]);
  const boundedLabelText = (root) => {
    const stack = [root];
    let stackLength = 1;
    let text = '';
    let visited = 0;
    while (stackLength !== 0) {
      if (visited >= MAX_LABEL_NODES) return null;
      visited += 1;
      stackLength -= 1;
      const current = stack[stackLength];
      const type = read(nodeTypeGet, current);
      if (type === 3) {
        const raw = read(characterDataGet, current);
        if (typeof raw !== 'string') return null;
        const separator = text === '' || raw === '' ? '' : ' ';
        const used = utf8Length(text, MAX_ATTRIBUTE_BYTES + 1);
        if (used > MAX_ATTRIBUTE_BYTES || used + separator.length > MAX_ATTRIBUTE_BYTES) return null;
        const bounded = boundedMetadata(raw, MAX_ATTRIBUTE_BYTES - used - separator.length);
        if (bounded === null) return null;
        if (bounded !== '') text += separator + bounded;
        continue;
      }
      if (type !== 1 && type !== 9 && type !== 11) return null;
      const children = read(nodeChildNodesGet, current);
      const length = listLength(children);
      if (length < 0 || length + stackLength + visited > MAX_LABEL_NODES) return null;
      for (let index = length - 1; index >= 0; index -= 1) {
        const child = listItem(children, index);
        if (child === null) return null;
        stack[stackLength] = child;
        stackLength += 1;
      }
    }
    return text;
  };
  const credentialLike = (target, isInput) => {
    let joined = '';
    const addMetadata = (raw, limit) => {
      if (raw === null || raw === '') return true;
      const bounded = boundedMetadata(raw, limit);
      if (bounded === null) return false;
      joined += ' ' + lowerText(bounded);
      return true;
    };
    const names = ['autocomplete', 'name', 'id', 'aria-label', 'placeholder', 'title'];
    const limits = [256, 256, 256, MAX_ATTRIBUTE_BYTES, MAX_ATTRIBUTE_BYTES, MAX_ATTRIBUTE_BYTES];
    for (let index = 0; index < names.length; index += 1) {
      let raw;
      try { raw = attr(target, names[index]); } catch (_) { return null; }
      if (raw !== null && typeof raw !== 'string') return null;
      if (!addMetadata(raw, limits[index])) return null;
    }
    let labelledBy;
    try { labelledBy = attr(target, 'aria-labelledby'); } catch (_) { return null; }
    if (labelledBy !== null) {
      if (!addMetadata(labelledBy, MAX_METADATA_ID_BYTES)) return null;
      const identifiers = apply(stringSplit, labelledBy, [/\s+/]);
      let identifierCount = 0;
      for (let index = 0; index < identifiers.length; index += 1) {
        const identifier = identifiers[index];
        if (identifier === '') continue;
        identifierCount += 1;
        if (identifierCount > 8 || identifier.length > 128) return null;
        const label = apply(documentGetElementById, mainDocument, [identifier]);
        if (label !== null) {
          const text = boundedLabelText(label);
          if (text === null || !addMetadata(text, MAX_ATTRIBUTE_BYTES)) return null;
        }
      }
    }
    let labels;
    try { labels = read(isInput ? inputLabelsGet : textareaLabelsGet, target); } catch (_) {
      return null;
    }
    if (labels !== null && labels !== undefined) {
      const length = listLength(labels);
      if (length < 0 || length > MAX_LABELS) return null;
      for (let index = 0; index < length; index += 1) {
        const label = listItem(labels, index);
        if (label === null) return null;
        const text = boundedLabelText(label);
        if (text === null || !addMetadata(text, MAX_ATTRIBUTE_BYTES)) return null;
      }
    }
    return (
      contains(joined, 'password') || contains(joined, 'passcode') ||
      contains(joined, 'one-time-code') || contains(joined, 'verification code') ||
      contains(joined, 'security code') || contains(joined, 'api key') ||
      contains(joined, 'access token') || contains(joined, 'secret key') ||
      contains(joined, 'private key') || contains(joined, 'cc-number') ||
      contains(joined, 'cc-csc') || contains(joined, 'card number') ||
      contains(joined, 'cvv') || contains(joined, 'cvc')
    );
  };
  const terminal = (target, attempt, status) => {
    removeAttr(target, COMMAND);
    setAttr(target, TERMINAL, `1|${attempt}|${status}`);
  };
  const observer = new NativeMutationObserver((records) => {
    if (!numberIsSafeInteger(records.length) || records.length < 1 || records.length > MAX_RECORDS) {
      return;
    }
    let candidateTarget = null;
    let candidateRaw = null;
    let candidateCount = 0;
    for (let index = 0; index < records.length; index += 1) {
      const record = records[index];
      let attributeName;
      let target;
      try {
        attributeName = read(mutationRecordAttributeNameGet, record);
        target = read(mutationRecordTargetGet, record);
      } catch (_) {
        return;
      }
      if (attributeName !== COMMAND || read(nodeTypeGet, target) !== 1) continue;
      const raw = attr(target, COMMAND);
      if (raw === null) continue;
      candidateCount += 1;
      if (candidateCount === 1) {
        candidateTarget = target;
        candidateRaw = raw;
      }
      if (candidateCount > 1) break;
    }
    if (candidateCount === 0) return;
    if (candidateCount !== 1) {
      terminal(candidateTarget, 0, 'duplicate');
      return;
    }
    const target = candidateTarget;
    const raw = candidateRaw;
    removeAttr(target, COMMAND);
    if (typeof raw !== 'string' || utf8Length(raw, MAX_COMMAND_BYTES + 1) > MAX_COMMAND_BYTES) {
      terminal(target, 0, 'invalid');
      return;
    }
    let command;
    try { command = apply(jsonParse, JSON, [raw]); } catch (_) {
      terminal(target, 0, 'invalid');
      return;
    }
    const keys = command !== null && typeof command === 'object' ? apply(objectKeys, Object, [command]) : [];
    const attempt = command !== null && typeof command === 'object' &&
      numberIsSafeInteger(command.a) && command.a > 0 && command.a <= MAX_SAFE_INTEGER
      ? command.a : 0;
    if (
      keys.length !== 3 || keys[0] !== 'v' || keys[1] !== 'a' || keys[2] !== 'z' ||
      command.v !== 1 || attempt === 0 ||
      typeof command.z !== 'string'
    ) {
      terminal(target, attempt, 'refused-command');
      return;
    }
    const prototype = getPrototypeOf(target);
    const isInput = prototype === inputPrototype;
    const isTextarea = prototype === textareaPrototype;
    if (
      (!isInput && !isTextarea) ||
      read(nodeOwnerDocumentGet, target) !== mainDocument ||
      read(nodeConnectedGet, target) !== true || apply(nodeRoot, target, []) !== mainDocument
    ) {
      terminal(target, attempt, 'refused-identity');
      return;
    }
    if (isInput && read(inputTypeGet, target) !== 'text' && read(inputTypeGet, target) !== 'search') {
      terminal(target, attempt, 'refused-type');
      return;
    }
    if (!validText(command.z, isTextarea)) {
      terminal(target, attempt, 'refused-command');
      return;
    }
    const disabledGet = isInput ? inputDisabledGet : textareaDisabledGet;
    const readOnlyGet = isInput ? inputReadOnlyGet : textareaReadOnlyGet;
    const valueSet = isInput ? inputValueSet : textareaValueSet;
    let credential;
    try { credential = credentialLike(target, isInput); } catch (_) { credential = null; }
    if (read(disabledGet, target) === true || read(readOnlyGet, target) === true) {
      terminal(target, attempt, 'refused-state');
      return;
    }
    if (credential !== false) {
      terminal(target, attempt, 'refused-credential');
      return;
    }
    let beforeEvent;
    let inputEvent;
    try {
      beforeEvent = new NativeInputEvent('beforeinput', {
        bubbles: true, cancelable: true, composed: true, data: command.z,
        inputType: 'insertReplacementText', isComposing: false
      });
      inputEvent = new NativeInputEvent('input', {
        bubbles: true, cancelable: false, composed: true, data: command.z,
        inputType: 'insertReplacementText', isComposing: false
      });
    } catch (_) {
      terminal(target, attempt, 'refused-construct');
      return;
    }
    try {
      if (apply(dispatchEvent, target, [beforeEvent]) !== true) {
        // The page observed `beforeinput` and may have produced effects even
        // when it cancelled the edit. Native must never classify this as a
        // clean retryable refusal.
        terminal(target, attempt, 'indeterminate');
        return;
      }
      if (
        getPrototypeOf(target) !== prototype ||
        read(nodeOwnerDocumentGet, target) !== mainDocument ||
        read(nodeConnectedGet, target) !== true || apply(nodeRoot, target, []) !== mainDocument ||
        (isInput && read(inputTypeGet, target) !== 'text' && read(inputTypeGet, target) !== 'search') ||
        read(disabledGet, target) === true || read(readOnlyGet, target) === true ||
        credentialLike(target, isInput) !== false || !validText(command.z, isTextarea)
      ) {
        terminal(target, attempt, 'indeterminate');
        return;
      }
    } catch (_) {
      terminal(target, attempt, 'indeterminate');
      return;
    }
    // Once the setter is entered the page may have changed even if WebKit
    // throws. Every subsequent failure is therefore indeterminate.
    try {
      apply(valueSet, target, [command.z]);
      apply(dispatchEvent, target, [inputEvent]);
      terminal(target, attempt, 'ok');
    } catch (_) {
      terminal(target, attempt, 'indeterminate');
    }
  });
  if (
    typeof mutationRecordTargetGet !== 'function' ||
    typeof mutationRecordAttributeNameGet !== 'function' ||
    typeof NativeMutationObserver !== 'function' || typeof observerObserve !== 'function' ||
    typeof observerDisconnect !== 'function' ||
    typeof documentElementGet !== 'function' ||
    typeof documentDefaultViewGet !== 'function' ||
    typeof documentGetElementById !== 'function' || typeof stringSplit !== 'function' ||
    typeof nodeTypeGet !== 'function' || typeof nodeOwnerDocumentGet !== 'function' ||
    typeof nodeChildNodesGet !== 'function' || typeof characterDataGet !== 'function' ||
    typeof nodeListLengthGet !== 'function' || typeof nodeListItem !== 'function' ||
    typeof nodeConnectedGet !== 'function' || typeof nodeRoot !== 'function' ||
    typeof inputValueSet !== 'function' ||
    typeof inputTypeGet !== 'function' || typeof inputDisabledGet !== 'function' ||
    typeof inputReadOnlyGet !== 'function' || typeof inputLabelsGet !== 'function' ||
    typeof NativeInputEvent !== 'function' ||
    typeof textareaValueSet !== 'function' || typeof textareaDisabledGet !== 'function' ||
    typeof textareaReadOnlyGet !== 'function' || typeof textareaLabelsGet !== 'function' ||
    typeof addEventListener !== 'function' || typeof removeEventListener !== 'function' ||
    read(documentDefaultViewGet, mainDocument) !== mainGlobal
  ) return;
  apply(observerObserve, observer, [mainDocument, {
    attributes: true, subtree: true, attributeFilter: [COMMAND]
  }]);
  let bootstrapObserver = null;
  let bootstrapListening = false;
  let bootstrapStopped = false;
  const stopBootstrap = () => {
    if (bootstrapStopped) return;
    bootstrapStopped = true;
    const currentObserver = bootstrapObserver;
    bootstrapObserver = null;
    if (currentObserver !== null) {
      try { apply(observerDisconnect, currentObserver, []); } catch (_) {}
    }
    if (bootstrapListening) {
      bootstrapListening = false;
      try {
        apply(removeEventListener, mainDocument, ['readystatechange', markReady]);
      } catch (_) {}
    }
  };
  const markReady = () => {
    if (bootstrapStopped) return;
    let root;
    try { root = read(documentElementGet, mainDocument); } catch (_) {
      stopBootstrap();
      return;
    }
    if (root === null) return;
    try { setAttr(root, READY, '1'); } catch (_) {
      stopBootstrap();
      return;
    }
    stopBootstrap();
  };
  markReady();
  if (!bootstrapStopped) {
    bootstrapObserver = new NativeMutationObserver((records) => {
      if (bootstrapStopped) return;
      if (
        !numberIsSafeInteger(records.length) ||
        records.length < 1 || records.length > MAX_BOOTSTRAP_RECORDS
      ) {
        stopBootstrap();
        return;
      }
      markReady();
    });
    apply(observerObserve, bootstrapObserver, [mainDocument, { childList: true }]);
    bootstrapListening = true;
    apply(addEventListener, mainDocument, ['readystatechange', markReady]);
    // Close the gap between the first root read and observer registration.
    markReady();
  }
})();"#;
static NEXT_SEMANTIC_RUNTIME_WORLD: AtomicU64 = AtomicU64::new(1);

type ReplyBlock = RcBlock<dyn Fn(*mut AnyObject, *mut NSString)>;
type SemanticCompletion = Box<dyn FnOnce(Result<SemanticSnapshot, AgentSemanticRuntimeFailure>)>;
type SemanticActionCompletion =
    Box<dyn FnOnce(Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>)>;

/// Synchronous refusal before one exact invocation enters the native channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentSemanticRuntimeDispatchError {
    NotReady,
    Busy,
    Exhausted,
    Retired,
}

/// Closed terminal failure for one admitted semantic invocation.
#[derive(Debug)]
pub(crate) enum AgentSemanticRuntimeFailure {
    Dispatch(AgentSemanticRuntimeDispatchError),
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
    Result(SemanticRuntimeResultError),
}

/// Closed terminal failure for one action-target revalidation invocation.
#[derive(Debug)]
pub(crate) enum AgentSemanticActionRuntimeFailure {
    Dispatch(AgentSemanticRuntimeDispatchError),
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
    Result(SemanticActionRuntimeResultError),
}

#[derive(Clone, Copy)]
enum RuntimeChannelFailure {
    Cancelled,
    DocumentReplaced,
    RendererLost,
    TimedOut,
    Retired,
    Transport,
}

impl RuntimeChannelFailure {
    const fn observation(self) -> AgentSemanticRuntimeFailure {
        match self {
            Self::Cancelled => AgentSemanticRuntimeFailure::Cancelled,
            Self::DocumentReplaced => AgentSemanticRuntimeFailure::DocumentReplaced,
            Self::RendererLost => AgentSemanticRuntimeFailure::RendererLost,
            Self::TimedOut => AgentSemanticRuntimeFailure::TimedOut,
            Self::Retired => AgentSemanticRuntimeFailure::Retired,
            Self::Transport => AgentSemanticRuntimeFailure::Transport,
        }
    }

    const fn action(self) -> AgentSemanticActionRuntimeFailure {
        match self {
            Self::Cancelled => AgentSemanticActionRuntimeFailure::Cancelled,
            Self::DocumentReplaced => AgentSemanticActionRuntimeFailure::DocumentReplaced,
            Self::RendererLost => AgentSemanticActionRuntimeFailure::RendererLost,
            Self::TimedOut => AgentSemanticActionRuntimeFailure::TimedOut,
            Self::Retired => AgentSemanticActionRuntimeFailure::Retired,
            Self::Transport => AgentSemanticActionRuntimeFailure::Transport,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DocumentPhase {
    Loading,
    Ready,
    RendererLost,
    ExhaustionNoticePending,
    Exhausted,
    Failed,
    Retired,
}

enum PendingInvocation {
    Observation {
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    },
    Action {
        invocation: SemanticActionRuntimeInvocation,
        completion: SemanticActionCompletion,
        authority: Option<Box<dyn Fn() -> bool>>,
    },
}

impl PendingInvocation {
    fn as_str(&self) -> &str {
        match self {
            Self::Observation { invocation, .. } => invocation.as_str(),
            Self::Action { invocation, .. } => invocation.as_str(),
        }
    }

    fn matches_observation(&self, invocation: zephium_agentic::SemanticInvocationId) -> bool {
        matches!(self, Self::Observation { invocation: current, .. } if current.invocation() == invocation)
    }

    fn matches_action(&self, attempt: SemanticActionAttemptId) -> bool {
        matches!(self, Self::Action { invocation, .. } if invocation.attempt() == attempt)
    }
}

enum ReplyValue {
    Success(Box<str>),
    Error,
}

struct ReplyAction {
    reply: ReplyBlock,
    value: ReplyValue,
}

impl ReplyAction {
    fn success(reply: ReplyBlock, value: impl Into<Box<str>>) -> Self {
        Self {
            reply,
            value: ReplyValue::Success(value.into()),
        }
    }

    fn error(reply: ReplyBlock) -> Self {
        Self {
            reply,
            value: ReplyValue::Error,
        }
    }
}

enum CompletionAction {
    Observation {
        completion: SemanticCompletion,
        outcome: Result<Box<SemanticSnapshot>, AgentSemanticRuntimeFailure>,
    },
    Action {
        completion: SemanticActionCompletion,
        outcome: Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>,
    },
}

#[cfg(test)]
impl CompletionAction {
    fn observation_outcome(self) -> Result<SemanticSnapshot, AgentSemanticRuntimeFailure> {
        match self {
            Self::Observation { outcome, .. } => outcome.map(|snapshot| *snapshot),
            Self::Action { .. } => panic!("expected observation completion"),
        }
    }
}

#[derive(Default)]
struct ChannelActions {
    first_reply: Option<ReplyAction>,
    second_reply: Option<ReplyAction>,
    completion: Option<CompletionAction>,
    invariant_failed: bool,
}

impl ChannelActions {
    fn push_reply(&mut self, reply: ReplyAction) {
        if self.first_reply.is_none() {
            self.first_reply = Some(reply);
        } else if self.second_reply.is_none() {
            self.second_reply = Some(reply);
        } else {
            self.invariant_failed = true;
        }
    }
}

struct SemanticRuntimeChannelState {
    expected_view: Option<usize>,
    active_world: Option<usize>,
    phase: DocumentPhase,
    pull: Option<ReplyBlock>,
    pending: Option<PendingInvocation>,
    awaiting_result: bool,
    completed_invocations: u16,
}

impl Default for SemanticRuntimeChannelState {
    fn default() -> Self {
        Self {
            expected_view: None,
            active_world: None,
            phase: DocumentPhase::Loading,
            pull: None,
            pending: None,
            awaiting_result: false,
            completed_invocations: 0,
        }
    }
}

impl SemanticRuntimeChannelState {
    fn bind_world(&mut self, world: &WKContentWorld) -> Result<(), ()> {
        if self.phase != DocumentPhase::Loading || self.active_world.is_some() {
            return Err(());
        }
        self.active_world = Some(std::ptr::from_ref(world).addr());
        Ok(())
    }

    fn world_matches(&self, world: &WKContentWorld) -> bool {
        self.active_world == Some(std::ptr::from_ref(world).addr())
    }

    fn bind_view(&mut self, view: &WKWebView) -> Result<(), ()> {
        let pointer = std::ptr::from_ref(view).addr();
        match self.expected_view {
            None if self.phase == DocumentPhase::Loading => {
                self.expected_view = Some(pointer);
                Ok(())
            }
            Some(expected) if expected == pointer => Ok(()),
            None | Some(_) => Err(()),
        }
    }

    fn dispatch_observation(
        &mut self,
        invocation: SemanticRuntimeInvocation,
        completion: SemanticCompletion,
    ) -> Result<ChannelActions, (AgentSemanticRuntimeDispatchError, SemanticCompletion)> {
        if let Some(failure) = self.admission_failure() {
            return Err((failure, completion));
        }
        self.pending = Some(PendingInvocation::Observation {
            invocation,
            completion,
        });
        Ok(self.prepare_pump())
    }

    fn dispatch_action(
        &mut self,
        invocation: SemanticActionRuntimeInvocation,
        completion: SemanticActionCompletion,
        authority: Option<Box<dyn Fn() -> bool>>,
    ) -> Result<ChannelActions, (AgentSemanticRuntimeDispatchError, SemanticActionCompletion)> {
        if let Some(failure) = self.admission_failure() {
            return Err((failure, completion));
        }
        self.pending = Some(PendingInvocation::Action {
            invocation,
            completion,
            authority,
        });
        Ok(self.prepare_pump())
    }

    fn admission_failure(&mut self) -> Option<AgentSemanticRuntimeDispatchError> {
        if self.expected_view.is_none() {
            return Some(AgentSemanticRuntimeDispatchError::NotReady);
        }
        match self.phase {
            DocumentPhase::Ready => {}
            DocumentPhase::Loading => {
                return Some(AgentSemanticRuntimeDispatchError::NotReady);
            }
            DocumentPhase::ExhaustionNoticePending | DocumentPhase::Exhausted => {
                return Some(AgentSemanticRuntimeDispatchError::Exhausted);
            }
            DocumentPhase::RendererLost | DocumentPhase::Failed | DocumentPhase::Retired => {
                return Some(AgentSemanticRuntimeDispatchError::Retired);
            }
        }
        if self.pending.is_some() || self.awaiting_result {
            return Some(AgentSemanticRuntimeDispatchError::Busy);
        }
        if self.completed_invocations >= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
            self.phase = DocumentPhase::Exhausted;
            return Some(AgentSemanticRuntimeDispatchError::Exhausted);
        }
        None
    }

    fn document_committed(&mut self) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if self.phase == DocumentPhase::Loading {
            self.phase = DocumentPhase::Ready;
        } else {
            actions = self.invalidate_current(RuntimeChannelFailure::DocumentReplaced);
            actions.invariant_failed = true;
            if self.phase != DocumentPhase::Retired {
                self.phase = DocumentPhase::Failed;
            }
        }
        // Wry may report the construction-only about:blank commit while the
        // builder still owns the new WKWebView and before `bind_view` can run.
        // Dispatch remains closed until the exact returned view is bound.
        actions.invariant_failed |= self.active_world.is_none();
        actions
    }

    fn begin_document_load(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::DocumentReplaced);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Loading;
            self.completed_invocations = 0;
            self.active_world = None;
        }
        actions
    }

    fn registration_failed(&mut self) -> ChannelActions {
        let mut actions = self.invalidate_current(RuntimeChannelFailure::Transport);
        self.active_world = None;
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn renderer_lost(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::RendererLost);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::RendererLost;
        }
        actions
    }

    fn cancel(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::Cancelled);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        actions
    }

    fn timeout(
        &mut self,
        invocation: zephium_agentic::SemanticInvocationId,
    ) -> (ChannelActions, bool) {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.matches_observation(invocation))
        {
            return (ChannelActions::default(), false);
        }
        let actions = self.invalidate_current(RuntimeChannelFailure::TimedOut);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        (actions, true)
    }

    fn timeout_action(&mut self, attempt: SemanticActionAttemptId) -> (ChannelActions, bool) {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| !pending.matches_action(attempt))
        {
            return (ChannelActions::default(), false);
        }
        let actions = self.invalidate_current(RuntimeChannelFailure::TimedOut);
        if self.phase != DocumentPhase::Retired {
            self.phase = DocumentPhase::Failed;
        }
        (actions, true)
    }

    fn retire(&mut self) -> ChannelActions {
        let actions = self.invalidate_current(RuntimeChannelFailure::Retired);
        self.active_world = None;
        self.phase = DocumentPhase::Retired;
        actions
    }

    fn fail_transport(&mut self, current: Option<ReplyBlock>) -> ChannelActions {
        let mut actions = self.invalidate_current(RuntimeChannelFailure::Transport);
        if let Some(reply) = current {
            actions.push_reply(ReplyAction::error(reply));
        }
        self.phase = DocumentPhase::Failed;
        actions.invariant_failed = true;
        actions
    }

    fn invalidate_current(&mut self, failure: RuntimeChannelFailure) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if let Some(pull) = self.pull.take() {
            actions.push_reply(ReplyAction::success(pull, SEMANTIC_RUNTIME_CHANNEL_STOP));
        }
        self.awaiting_result = false;
        if let Some(pending) = self.pending.take() {
            actions.completion = Some(match pending {
                PendingInvocation::Observation { completion, .. } => {
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(failure.observation()),
                    }
                }
                PendingInvocation::Action { completion, .. } => CompletionAction::Action {
                    completion,
                    outcome: Err(failure.action()),
                },
            });
        }
        actions
    }

    fn on_message(&mut self, body: &str, reply: ReplyBlock) -> ChannelActions {
        if body == SEMANTIC_RUNTIME_CHANNEL_PULL {
            return self.on_pull(reply);
        }
        if body == SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED {
            return self.on_exhausted(reply);
        }
        if let Some(result) = body.strip_prefix(SEMANTIC_RUNTIME_CHANNEL_RESULT_PREFIX) {
            return self.on_result(result.as_bytes(), reply);
        }
        self.fail_transport(Some(reply))
    }

    fn on_pull(&mut self, reply: ReplyBlock) -> ChannelActions {
        if matches!(
            self.phase,
            DocumentPhase::RendererLost
                | DocumentPhase::ExhaustionNoticePending
                | DocumentPhase::Exhausted
                | DocumentPhase::Failed
                | DocumentPhase::Retired
        ) {
            let mut actions = ChannelActions::default();
            actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_STOP));
            return actions;
        }
        if self.pull.is_some() || self.awaiting_result {
            return self.fail_transport(Some(reply));
        }
        self.pull = Some(reply);
        self.prepare_pump()
    }

    fn on_result(&mut self, bytes: &[u8], reply: ReplyBlock) -> ChannelActions {
        if !self.awaiting_result {
            return self.fail_transport(Some(reply));
        }
        let Some(pending) = self.pending.take() else {
            return self.fail_transport(Some(reply));
        };
        self.awaiting_result = false;
        let Some(completed) = self.completed_invocations.checked_add(1) else {
            let mut actions = self.fail_transport(Some(reply));
            actions.completion = Some(match pending {
                PendingInvocation::Observation { completion, .. } => {
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(AgentSemanticRuntimeFailure::Transport),
                    }
                }
                PendingInvocation::Action { completion, .. } => CompletionAction::Action {
                    completion,
                    outcome: Err(AgentSemanticActionRuntimeFailure::Transport),
                },
            });
            return actions;
        };
        self.completed_invocations = completed;
        if completed == MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS {
            self.phase = DocumentPhase::ExhaustionNoticePending;
        }

        let (completion, recoverable) = match pending {
            PendingInvocation::Observation {
                invocation,
                completion,
            } => {
                let outcome = invocation
                    .decode_result(bytes)
                    .map(Box::new)
                    .map_err(AgentSemanticRuntimeFailure::Result);
                let recoverable = matches!(
                    &outcome,
                    Ok(_)
                        | Err(AgentSemanticRuntimeFailure::Result(
                            SemanticRuntimeResultError::Runtime(_)
                        ))
                );
                (
                    CompletionAction::Observation {
                        completion,
                        outcome,
                    },
                    recoverable,
                )
            }
            PendingInvocation::Action {
                invocation,
                completion,
                ..
            } => {
                let outcome = invocation
                    .decode_result(bytes)
                    .map_err(AgentSemanticActionRuntimeFailure::Result);
                let recoverable = matches!(
                    &outcome,
                    Ok(_)
                        | Err(AgentSemanticActionRuntimeFailure::Result(
                            SemanticActionRuntimeResultError::Runtime(_)
                        ))
                );
                (
                    CompletionAction::Action {
                        completion,
                        outcome,
                    },
                    recoverable,
                )
            }
        };
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(
            reply,
            if recoverable {
                SEMANTIC_RUNTIME_CHANNEL_ACK
            } else {
                SEMANTIC_RUNTIME_CHANNEL_STOP
            },
        ));
        actions.completion = Some(completion);
        if !recoverable {
            self.phase = DocumentPhase::Failed;
            actions.invariant_failed = true;
        }
        actions
    }

    fn on_exhausted(&mut self, reply: ReplyBlock) -> ChannelActions {
        if self.completed_invocations != MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            || self.pending.is_some()
            || self.awaiting_result
            || self.pull.is_some()
            || self.phase != DocumentPhase::ExhaustionNoticePending
        {
            return self.fail_transport(Some(reply));
        }
        self.phase = DocumentPhase::Exhausted;
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_ACK));
        actions
    }

    fn prepare_pump(&mut self) -> ChannelActions {
        let mut actions = ChannelActions::default();
        if self.phase != DocumentPhase::Ready
            || self.awaiting_result
            || self.pending.is_none()
            || self.pull.is_none()
        {
            return actions;
        }
        // A retained recipe can wait for the page's next pull. Recheck the
        // native owner at the actual handoff, before exposing any effect bytes.
        if self.pending.as_ref().is_some_and(|pending| {
            matches!(pending,
                PendingInvocation::Action { authority: Some(authority), .. } if !authority()
            )
        }) {
            return self.cancel();
        }
        let request = self
            .pending
            .as_ref()
            .map(|pending| pending.as_str().to_owned().into_boxed_str());
        let Some(request) = request else {
            actions.invariant_failed = true;
            return actions;
        };
        let Some(pull) = self.pull.take() else {
            actions.invariant_failed = true;
            return actions;
        };
        self.awaiting_result = true;
        actions.push_reply(ReplyAction::success(pull, request));
        actions
    }
}

#[derive(Clone)]
pub(crate) struct AgentSemanticRuntimeController {
    state: Rc<RefCell<SemanticRuntimeChannelState>>,
    on_invariant_failure: Rc<dyn Fn()>,
    on_callback_panic: Rc<dyn Fn()>,
}

impl AgentSemanticRuntimeController {
    fn new(on_invariant_failure: Rc<dyn Fn()>, on_callback_panic: Rc<dyn Fn()>) -> Self {
        Self {
            state: Rc::new(RefCell::new(SemanticRuntimeChannelState::default())),
            on_invariant_failure,
            on_callback_panic,
        }
    }

    pub(crate) fn bind_view(&self, view: &WKWebView) -> Result<(), ()> {
        self.state.try_borrow_mut().map_err(|_| ())?.bind_view(view)
    }

    fn bind_world(&self, world: &WKContentWorld) -> Result<(), ()> {
        self.state
            .try_borrow_mut()
            .map_err(|_| ())?
            .bind_world(world)
    }

    fn world_matches(&self, world: &WKContentWorld) -> Result<bool, ()> {
        Ok(self
            .state
            .try_borrow()
            .map_err(|_| ())?
            .world_matches(world))
    }

    pub(crate) fn dispatch(
        &self,
        invocation: SemanticRuntimeInvocation,
        completion: impl FnOnce(Result<SemanticSnapshot, AgentSemanticRuntimeFailure>) + 'static,
    ) -> Result<(), AgentSemanticRuntimeDispatchError> {
        let completion: SemanticCompletion = Box::new(completion);
        let dispatched = match self.state.try_borrow_mut() {
            Ok(mut state) => state.dispatch_observation(invocation, completion),
            Err(_) => Err((AgentSemanticRuntimeDispatchError::Busy, completion)),
        };
        match dispatched {
            Ok(actions) => {
                self.execute(actions);
                Ok(())
            }
            Err((failure, completion)) => {
                invoke_completion(
                    CompletionAction::Observation {
                        completion,
                        outcome: Err(AgentSemanticRuntimeFailure::Dispatch(failure)),
                    },
                    self.on_callback_panic.as_ref(),
                );
                Err(failure)
            }
        }
    }

    pub(crate) fn dispatch_action_guarded(
        &self,
        invocation: SemanticActionRuntimeInvocation,
        authority: Option<Box<dyn Fn() -> bool>>,
        completion: impl FnOnce(Result<SemanticActionRuntimeEvidence, AgentSemanticActionRuntimeFailure>)
            + 'static,
    ) -> Result<(), AgentSemanticRuntimeDispatchError> {
        let completion: SemanticActionCompletion = Box::new(completion);
        let dispatched = match self.state.try_borrow_mut() {
            Ok(mut state) => state.dispatch_action(invocation, completion, authority),
            Err(_) => Err((AgentSemanticRuntimeDispatchError::Busy, completion)),
        };
        match dispatched {
            Ok(actions) => {
                self.execute(actions);
                Ok(())
            }
            Err((failure, completion)) => {
                invoke_completion(
                    CompletionAction::Action {
                        completion,
                        outcome: Err(AgentSemanticActionRuntimeFailure::Dispatch(failure)),
                    },
                    self.on_callback_panic.as_ref(),
                );
                Err(failure)
            }
        }
    }

    pub(crate) fn begin_document_load(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::begin_document_load);
        self.execute(actions);
    }

    fn registration_failed(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::registration_failed);
        self.execute(actions);
    }

    pub(crate) fn document_committed(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::document_committed);
        self.execute(actions);
    }

    pub(crate) fn renderer_lost(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::renderer_lost);
        self.execute(actions);
    }

    pub(crate) fn cancel(&self) {
        let actions = self.transition(SemanticRuntimeChannelState::cancel);
        self.execute(actions);
    }

    pub(crate) fn timeout(&self, invocation: zephium_agentic::SemanticInvocationId) -> bool {
        let (actions, matched) = match self.state.try_borrow_mut() {
            Ok(mut state) => state.timeout(invocation),
            Err(_) => (
                ChannelActions {
                    invariant_failed: true,
                    ..ChannelActions::default()
                },
                false,
            ),
        };
        self.execute(actions);
        matched
    }

    pub(crate) fn timeout_action(&self, attempt: SemanticActionAttemptId) -> bool {
        let (actions, matched) = match self.state.try_borrow_mut() {
            Ok(mut state) => state.timeout_action(attempt),
            Err(_) => (
                ChannelActions {
                    invariant_failed: true,
                    ..ChannelActions::default()
                },
                false,
            ),
        };
        self.execute(actions);
        matched
    }

    pub(crate) fn pending_for_audit(&self) -> Option<bool> {
        let state = self.state.try_borrow().ok()?;
        let pending = state.pending.is_some();
        let world_valid = if state.phase == DocumentPhase::Retired {
            state.active_world.is_none()
        } else {
            state.active_world.is_some()
        };
        let valid = world_valid
            && state.completed_invocations <= MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS
            && (!state.awaiting_result || (pending && state.pull.is_none()))
            && (state.pull.is_none() || (!state.awaiting_result && !pending))
            && (!matches!(
                state.phase,
                DocumentPhase::RendererLost
                    | DocumentPhase::ExhaustionNoticePending
                    | DocumentPhase::Exhausted
                    | DocumentPhase::Failed
                    | DocumentPhase::Retired
            ) || (!pending && !state.awaiting_result && state.pull.is_none()));
        valid.then_some(pending)
    }

    // Private debug evidence only. Addresses never leave the in-memory witness.
    #[cfg(feature = "native-agentic-work-resource-probe")]
    pub(crate) fn witness_identity(&self) -> Option<(usize, usize, u16)> {
        if self.pending_for_audit() != Some(false) {
            return None;
        }
        let state = self.state.try_borrow().ok()?;
        if state.phase != DocumentPhase::Ready {
            return None;
        }
        Some((
            state.expected_view?,
            state.active_world?,
            state.completed_invocations,
        ))
    }

    fn retire(&self) -> bool {
        let (actions, clean) = match self.state.try_borrow_mut() {
            Ok(mut state) => (state.retire(), true),
            Err(_) => {
                let actions = ChannelActions {
                    invariant_failed: true,
                    ..ChannelActions::default()
                };
                (actions, false)
            }
        };
        self.execute(actions);
        clean
    }

    fn transition(
        &self,
        transition: fn(&mut SemanticRuntimeChannelState) -> ChannelActions,
    ) -> ChannelActions {
        self.state.try_borrow_mut().map_or_else(
            |_| ChannelActions {
                invariant_failed: true,
                ..ChannelActions::default()
            },
            |mut state| transition(&mut state),
        )
    }

    fn expected_view(&self) -> Option<usize> {
        self.state.try_borrow().ok()?.expected_view
    }

    fn receive(&self, body: &str, reply: ReplyBlock) {
        let actions = match self.state.try_borrow_mut() {
            Ok(mut state) => state.on_message(body, reply),
            Err(_) => {
                let mut actions = ChannelActions::default();
                actions.push_reply(ReplyAction::error(reply));
                actions.invariant_failed = true;
                actions
            }
        };
        self.execute(actions);
    }

    fn reject(&self, reply: ReplyBlock, invariant: bool) {
        let mut actions = ChannelActions::default();
        actions.push_reply(ReplyAction::success(reply, SEMANTIC_RUNTIME_CHANNEL_STOP));
        actions.invariant_failed = invariant;
        self.execute(actions);
    }

    fn execute(&self, mut actions: ChannelActions) {
        let mut reply_failed = false;
        for reply in [actions.first_reply.take(), actions.second_reply.take()]
            .into_iter()
            .flatten()
        {
            reply_failed |= !send_reply(reply);
        }
        if actions.invariant_failed || reply_failed {
            invoke_unit_callback(
                self.on_invariant_failure.as_ref(),
                self.on_callback_panic.as_ref(),
            );
        }
        if reply_failed {
            let mut failure = self.state.try_borrow_mut().map_or_else(
                |_| ChannelActions::default(),
                |mut state| state.fail_transport(None),
            );
            for reply in [failure.first_reply.take(), failure.second_reply.take()]
                .into_iter()
                .flatten()
            {
                let _ = send_reply(reply);
            }
            if let Some(completion) = failure.completion.take() {
                invoke_completion(completion, self.on_callback_panic.as_ref());
            }
        }
        if let Some(completion) = actions.completion.take() {
            invoke_completion(completion, self.on_callback_panic.as_ref());
        }
    }
}

fn invoke_completion(completion: CompletionAction, on_panic: &dyn Fn()) {
    if std::panic::catch_unwind(AssertUnwindSafe(|| match completion {
        CompletionAction::Observation {
            completion,
            outcome,
        } => completion(outcome.map(|snapshot| *snapshot)),
        CompletionAction::Action {
            completion,
            outcome,
        } => completion(outcome),
    }))
    .is_err()
    {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(on_panic));
    }
}

fn invoke_unit_callback(callback: &dyn Fn(), on_panic: &dyn Fn()) {
    if std::panic::catch_unwind(AssertUnwindSafe(callback)).is_err() {
        let _ = std::panic::catch_unwind(AssertUnwindSafe(on_panic));
    }
}

fn send_reply(action: ReplyAction) -> bool {
    objc2::exception::catch(AssertUnwindSafe(|| match action.value {
        ReplyValue::Success(value) => {
            let value = NSString::from_str(&value);
            let pointer = Retained::as_ptr(&value).cast_mut().cast::<AnyObject>();
            action.reply.call((pointer, null_mut()));
        }
        ReplyValue::Error => {
            let error = NSString::from_str(SEMANTIC_RUNTIME_FIXED_ERROR);
            action
                .reply
                .call((null_mut(), Retained::as_ptr(&error).cast_mut()));
        }
    }))
    .is_ok()
}

struct SemanticMessageHandlerIvars {
    controller: Retained<WKUserContentController>,
    world: Retained<WKContentWorld>,
    handler_name: Retained<NSString>,
    channel: AgentSemanticRuntimeController,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ZephiumSemanticRuntimeMessageHandler"]
    #[ivars = SemanticMessageHandlerIvars]
    struct SemanticMessageHandler;

    // SAFETY: `define_class!` fixes `NSObject` as this class's superclass and
    // objc2 initializes the declared ivars before exposing an instance.
    unsafe impl NSObjectProtocol for SemanticMessageHandler {}

    // SAFETY: the registered selector and Rust parameters exactly implement
    // WebKit's generated reply-handler protocol. `MainThreadOnly` prevents the
    // object from crossing threads, and callback state is retained in ivars.
    unsafe impl WKScriptMessageHandlerWithReply for SemanticMessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:replyHandler:))]
        unsafe fn user_content_controller_did_receive_script_message_reply_handler(
            &self,
            controller: &WKUserContentController,
            message: &WKScriptMessage,
            reply: &block2::DynBlock<dyn Fn(*mut AnyObject, *mut NSString)>,
        ) {
            let ivars = self.ivars();
            let reply = reply.copy();
            let expected_view = ivars.channel.expected_view();
            // SAFETY: WebKit supplied live protocol callback objects for this
            // exact selector; objc2 retains each object returned by the four
            // property messages for the duration of the checks below.
            let (world, name, webview, frame) = unsafe {
                (
                    message.world(),
                    message.name(),
                    message.webView(),
                    message.frameInfo(),
                )
            };
            if !std::ptr::eq(controller, &*ivars.controller)
                || Retained::as_ptr(&world) != Retained::as_ptr(&ivars.world)
                || !name.isEqualToString(&ivars.handler_name)
            {
                ivars.channel.reject(reply, true);
                return;
            }
            match ivars.channel.world_matches(&world) {
                Ok(true) => {}
                // Removing a document's world cannot retract a callback that
                // WebKit already delivered. That old epoch has no authority.
                Ok(false) => {
                    ivars.channel.reject(reply, false);
                    return;
                }
                Err(()) => {
                    ivars.channel.reject(reply, true);
                    return;
                }
            }
            let Some(expected_view) = expected_view else {
                // The construction-only about:blank can execute before the
                // returned WKWebView is bound. It receives no authority and
                // exits; later documents install a fresh runtime.
                ivars.channel.reject(reply, false);
                return;
            };
            let view_matches = webview
                .as_ref()
                .is_some_and(|view| std::ptr::from_ref(&**view).addr() == expected_view);
            // SAFETY: `frame` is the live retained WKFrameInfo obtained from
            // this callback message and this MainThreadOnly handler is running
            // on WebKit's main-thread delivery path.
            let (frame_webview, is_main_frame) = unsafe { (frame.webView(), frame.isMainFrame()) };
            let frame_view_matches = frame_webview
                .as_ref()
                .is_some_and(|view| std::ptr::from_ref(&**view).addr() == expected_view);
            if !view_matches || !frame_view_matches || !is_main_frame {
                ivars.channel.reject(reply, true);
                return;
            }
            // SAFETY: `message` remains live for this protocol callback; objc2
            // retains its Objective-C body before the type and size checks.
            let body = unsafe { message.body() };
            let Ok(body) = body.downcast::<NSString>() else {
                ivars.channel.reject(reply, true);
                return;
            };
            if body.length() > MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES
                || body.lengthOfBytesUsingEncoding(NSUTF8StringEncoding)
                    > MAX_SEMANTIC_RUNTIME_CHANNEL_RESULT_BYTES
            {
                ivars.channel.reject(reply, true);
                return;
            }
            ivars.channel.receive(&body.to_string(), reply);
        }
    }
);

struct SemanticRuntimeEpochRegistration {
    world: Retained<WKContentWorld>,
    script: Retained<WKUserScript>,
    page_relay_world: Retained<WKContentWorld>,
    page_relay_script: Retained<WKUserScript>,
    handler: Retained<SemanticMessageHandler>,
}

fn next_semantic_runtime_world(mtm: MainThreadMarker) -> Result<Retained<WKContentWorld>, ()> {
    let identifier = NEXT_SEMANTIC_RUNTIME_WORLD
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map_err(|_| ())?;
    let name = NSString::from_str(&format!(
        "{SEMANTIC_RUNTIME_WORLD_NAME_PREFIX}{identifier:016x}"
    ));
    // SAFETY: `mtm` proves main-thread creation and `name` is a live retained
    // NSString. Objective-C exceptions are contained at this boundary.
    let world = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        WKContentWorld::worldWithName(&name, mtm)
    }))
    .map_err(|_| ())?;
    // SAFETY: `world` is the retained object returned above and access remains
    // on the main thread proven by `mtm`.
    if unsafe { world.name() }
        .as_ref()
        .is_none_or(|actual| !actual.isEqualToString(&name))
    {
        return Err(());
    }
    Ok(world)
}

fn clear_semantic_runtime_controller(
    controller: &WKUserContentController,
    handler_name: &NSString,
    world: Option<&WKContentWorld>,
) -> bool {
    if MainThreadMarker::new().is_none() {
        return false;
    }
    // SAFETY: the marker check above proves main-thread access; controller,
    // handler name, and optional world are live retained objects. Exceptions
    // are caught and failure leaves the caller in fail-closed cleanup.
    let removed = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        if let Some(world) = world {
            controller.removeScriptMessageHandlerForName_contentWorld(handler_name, world);
        }
        // This controller is created solely for the owned agent view. The
        // sweep makes a partial registration failure mechanically empty.
        controller.removeAllScriptMessageHandlers();
        controller.removeAllUserScripts();
    }))
    .is_ok();
    // SAFETY: `controller` remains live on the verified main thread; objc2
    // retains the returned script array for this bounded inventory check.
    removed && unsafe { controller.userScripts() }.count() == 0
}

fn install_semantic_runtime_epoch(
    controller: &WKUserContentController,
    handler_name: &NSString,
    channel: &AgentSemanticRuntimeController,
    mtm: MainThreadMarker,
) -> Result<SemanticRuntimeEpochRegistration, ()> {
    let world = next_semantic_runtime_world(mtm)?;
    let handler = SemanticMessageHandler::alloc(mtm).set_ivars(SemanticMessageHandlerIvars {
        controller: controller.retain(),
        world: world.clone(),
        handler_name: handler_name.retain(),
        channel: channel.clone(),
    });
    // SAFETY: `handler` is a freshly allocated instance of the declared class
    // with fully initialized ivars; `init` is NSObject's designated initializer.
    let handler: Retained<SemanticMessageHandler> = unsafe { msg_send![super(handler), init] };
    let protocol_handler = objc2::runtime::ProtocolObject::from_ref(&*handler);
    // SAFETY: `mtm` proves main-thread registration and every Objective-C
    // argument is retained for this call. WebKit retains the protocol handler;
    // exceptions are caught and trigger complete controller cleanup.
    let added_handler = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addScriptMessageHandlerWithReply_contentWorld_name(
            protocol_handler,
            &world,
            handler_name,
        );
    }))
    .is_ok();
    if !added_handler {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }

    let source = NSString::from_str(SEMANTIC_RUNTIME_PROGRAM.source());
    // SAFETY: `mtm` proves main-thread allocation; source and content world are
    // live retained values, and the fixed enum/bool arguments match WebKit's
    // initializer contract. Objective-C exceptions are contained.
    let script = match objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
            WKUserScript::alloc(mtm),
            &source,
            WKUserScriptInjectionTime::AtDocumentStart,
            true,
            &world,
        )
    })) {
        Ok(script) => script,
        Err(_) => {
            let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
            return Err(());
        }
    };
    // SAFETY: controller and script are live retained main-thread objects;
    // WebKit retains the script and any exception is converted to refusal.
    let added = objc2::exception::catch(AssertUnwindSafe(|| unsafe {
        controller.addUserScript(&script);
    }))
    .is_ok();
    if !added {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }
    // This registration is reachable only from the owned-agent-view
    // constructor. Installing the page-world compatibility shim is part of
    // that type's construction invariant, never an environment-selected
    // capability. Browse and borrowed tab configurations do not use this
    // registration at all.
    // SAFETY: `mtm` proves main-thread access; WebKit's page-world singleton,
    // fixed source, initializer arguments, and controller are live native
    // values. The controller retains the script, and Objective-C exceptions
    // are contained and fail the complete paired installation.
    let (page_relay_world, page_relay_script) =
        match objc2::exception::catch(AssertUnwindSafe(|| unsafe {
            let relay_world = WKContentWorld::pageWorld(mtm);
            let relay_source = NSString::from_str(PAGE_WORLD_COMPATIBILITY_FILL_PROGRAM);
            let relay_script =
                WKUserScript::initWithSource_injectionTime_forMainFrameOnly_inContentWorld(
                    WKUserScript::alloc(mtm),
                    &relay_source,
                    WKUserScriptInjectionTime::AtDocumentStart,
                    true,
                    &relay_world,
                );
            controller.addUserScript(&relay_script);
            (relay_world, relay_script)
        })) {
            Ok(installed) => installed,
            Err(_) => {
                let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
                return Err(());
            }
        };
    if channel.bind_world(&world).is_err() {
        let _ = clear_semantic_runtime_controller(controller, handler_name, Some(&world));
        return Err(());
    }
    Ok(SemanticRuntimeEpochRegistration {
        world,
        script,
        page_relay_world,
        page_relay_script,
        handler,
    })
}

/// Retains one production semantic world, handler, and document-start script.
///
/// The registration rotates to a fresh world before every authorized native
/// load. WebKit retains the posting JavaScript context until an asynchronous
/// reply is delivered, so a fixed world cannot distinguish a late callback
/// from the replaced document through public `WKFrameInfo` alone.
pub(crate) struct AgentSemanticRuntimeRegistration {
    controller: Retained<WKUserContentController>,
    handler_name: Retained<NSString>,
    active: Option<SemanticRuntimeEpochRegistration>,
    channel: AgentSemanticRuntimeController,
    retired: bool,
}

impl AgentSemanticRuntimeRegistration {
    pub(crate) fn install(
        configuration: &WKWebViewConfiguration,
        on_invariant_failure: Rc<dyn Fn()>,
        on_callback_panic: Rc<dyn Fn()>,
    ) -> Result<Self, ()> {
        let mtm = MainThreadMarker::new().ok_or(())?;
        // SAFETY: `mtm` proves main-thread access and `configuration` is a live
        // retained configuration supplied by the construction path. objc2
        // retains the returned controller and script inventory.
        let controller = unsafe { configuration.userContentController() };
        // SAFETY: the retained controller remains live on the same main thread.
        if unsafe { controller.userScripts() }.count() != 0 {
            return Err(());
        }
        let handler_name = NSString::from_str(SEMANTIC_RUNTIME_CHANNEL_NAME);
        let channel = AgentSemanticRuntimeController::new(on_invariant_failure, on_callback_panic);
        let active = install_semantic_runtime_epoch(&controller, &handler_name, &channel, mtm)?;

        let registration = Self {
            controller,
            handler_name,
            active: Some(active),
            channel,
            retired: false,
        };
        registration.attest_configuration(configuration)?;
        Ok(registration)
    }

    pub(crate) fn bind_view(&self, view: &WKWebView) -> Result<(), ()> {
        self.channel.bind_view(view)
    }

    pub(crate) const fn controller(&self) -> &AgentSemanticRuntimeController {
        &self.channel
    }

    /// Revokes the old document world and installs the immutable program in a
    /// fresh one before native navigation can begin.
    pub(crate) fn prepare_document_load(&mut self) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        self.channel.begin_document_load();
        let Some(old) = self.active.take() else {
            self.channel.registration_failed();
            return Err(());
        };
        if !clear_semantic_runtime_controller(
            &self.controller,
            &self.handler_name,
            Some(&old.world),
        ) {
            self.channel.registration_failed();
            return Err(());
        }
        drop(old);
        let Some(mtm) = MainThreadMarker::new() else {
            self.channel.registration_failed();
            return Err(());
        };
        match install_semantic_runtime_epoch(
            &self.controller,
            &self.handler_name,
            &self.channel,
            mtm,
        ) {
            Ok(active) => {
                self.active = Some(active);
                if self.attest_controller().is_ok() {
                    Ok(())
                } else {
                    let world = self.active.as_ref().map(|active| &*active.world);
                    let _ = clear_semantic_runtime_controller(
                        &self.controller,
                        &self.handler_name,
                        world,
                    );
                    self.active = None;
                    self.channel.registration_failed();
                    Err(())
                }
            }
            Err(()) => {
                self.channel.registration_failed();
                Err(())
            }
        }
    }

    pub(crate) fn attest_configuration(
        &self,
        configuration: &WKWebViewConfiguration,
    ) -> Result<(), ()> {
        if self.retired {
            return Err(());
        }
        let _mtm = MainThreadMarker::new().ok_or(())?;
        // SAFETY: the marker above proves main-thread access; `configuration`
        // is live and objc2 retains its returned content controller.
        let actual_controller = unsafe { configuration.userContentController() };
        if Retained::as_ptr(&actual_controller) != Retained::as_ptr(&self.controller) {
            return Err(());
        }
        self.attest_controller()
    }

    fn attest_controller(&self) -> Result<(), ()> {
        let _mtm = MainThreadMarker::new().ok_or(())?;
        let active = self.active.as_ref().ok_or(())?;
        if self.channel.world_matches(&active.world) != Ok(true) {
            return Err(());
        }
        // SAFETY: the marker above proves main-thread access; the retained
        // controller remains live and objc2 retains its script inventory.
        let scripts = unsafe { self.controller.userScripts() };
        if scripts.count() != 2 {
            return Err(());
        }
        let script = scripts.objectAtIndex(0);
        // SAFETY: the count check proves index zero exists, `script` is retained
        // by objc2, and all WebKit property reads remain on the main thread.
        let (source, injection_time, main_frame_only) = unsafe {
            (
                script.source(),
                script.injectionTime(),
                script.isForMainFrameOnly(),
            )
        };
        if Retained::as_ptr(&script) != Retained::as_ptr(&active.script)
            || source.to_string() != SEMANTIC_RUNTIME_PROGRAM.source()
            || injection_time != WKUserScriptInjectionTime::AtDocumentStart
            || !main_frame_only
        {
            return Err(());
        }
        let relay = scripts.objectAtIndex(1);
        // SAFETY: the exact inventory count proves index one exists.
        let (source, injection_time, main_frame_only) = unsafe {
            (
                relay.source(),
                relay.injectionTime(),
                relay.isForMainFrameOnly(),
            )
        };
        // SAFETY: this stays on the main thread and returns WebKit's retained
        // public page-world singleton for exact world identity checking.
        let actual_page_world =
            unsafe { WKContentWorld::pageWorld(MainThreadMarker::new().ok_or(())?) };
        if Retained::as_ptr(&relay) != Retained::as_ptr(&active.page_relay_script)
            || Retained::as_ptr(&active.page_relay_world) != Retained::as_ptr(&actual_page_world)
            || source.to_string() != PAGE_WORLD_COMPATIBILITY_FILL_PROGRAM
            || injection_time != WKUserScriptInjectionTime::AtDocumentStart
            || !main_frame_only
        {
            return Err(());
        }
        let _ = &active.handler;
        Ok(())
    }

    pub(crate) fn retire(mut self) -> Result<(), ()> {
        let channel_clean = self.channel.retire();
        let world = self.active.as_ref().map(|active| &*active.world);
        let removed =
            clear_semantic_runtime_controller(&self.controller, &self.handler_name, world);
        self.active = None;
        self.retired = true;
        if channel_clean && removed {
            Ok(())
        } else {
            Err(())
        }
    }
}

impl Drop for AgentSemanticRuntimeRegistration {
    fn drop(&mut self) {
        if self.retired {
            return;
        }
        let _ = self.channel.retire();
        let world = self.active.as_ref().map(|active| &*active.world);
        let _ = clear_semantic_runtime_controller(&self.controller, &self.handler_name, world);
        self.active = None;
        self.retired = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zephium_agentic::{
        encode_semantic_runtime_invocation, ContextCapabilities, ContextCapability, ContextId,
        ContextIdentity, ContextKind, ContextOperationId, ContextRegistry, ContextRunId,
        ContextSettlement, FrameId, SemanticFrameJoin, SemanticFrameTrust, SemanticInvocationId,
        SemanticObservationBudget, SemanticObservationId, SemanticObservationRequest,
        SemanticOrigin, SemanticRuntimeBudget, SemanticRuntimeFault, SemanticSnapshotGeneration,
    };

    #[test]
    fn retained_action_rechecks_authority_at_page_pull_before_releasing_recipe() {
        for revoked in [false, true] {
            let native = crate::agent_context_port::WorkActionTask::native_for_test();
            let invocation =
                zephium_agentic::encode_semantic_action_runtime_invocation(&native).unwrap();
            let encoded = invocation.as_str().to_owned();
            let allowed = Rc::new(std::cell::Cell::new(true));
            let fence = allowed.clone();
            let mut state = SemanticRuntimeChannelState {
                phase: DocumentPhase::Ready,
                expected_view: Some(7),
                active_world: Some(8),
                ..SemanticRuntimeChannelState::default()
            };
            let pending = state
                .dispatch_action(
                    invocation,
                    Box::new(|_| {}),
                    Some(Box::new(move || fence.get())),
                )
                .unwrap_or_else(|_| panic!("fixture admission"));
            assert!(pending.first_reply.is_none());
            assert!(pending.completion.is_none());
            allowed.set(!revoked);
            let mut actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply());
            let delivered = success_value(actions.first_reply.take().unwrap());
            if revoked {
                assert_eq!(&*delivered, SEMANTIC_RUNTIME_CHANNEL_STOP);
                assert!(matches!(
                    actions.completion,
                    Some(CompletionAction::Action {
                        outcome: Err(AgentSemanticActionRuntimeFailure::Cancelled),
                        ..
                    })
                ));
                assert!(state.pending.is_none());
                assert!(!state.awaiting_result);
                assert_eq!(state.phase, DocumentPhase::Failed);
            } else {
                assert_eq!(&*delivered, encoded);
                assert!(actions.completion.is_none());
                assert!(state.awaiting_result);
            }
        }
    }

    #[test]
    fn owned_view_fill_shim_is_fixed_bounded_and_has_no_native_authority() {
        let source = PAGE_WORLD_COMPATIBILITY_FILL_PROGRAM;
        assert!(source.len() <= 32 * 1024);
        assert!(source.contains("HTMLInputElement.prototype"));
        assert!(source.contains("HTMLTextAreaElement.prototype"));
        assert!(source.contains("MutationRecord.prototype"));
        assert!(source.contains("read(mutationRecordTargetGet, record)"));
        assert!(source.contains("Document.prototype.getElementById"));
        assert!(source.contains("getOwnDescriptor(inputPrototype, 'labels')"));
        assert!(source.contains("getOwnDescriptor(textareaPrototype, 'labels')"));
        assert!(source.contains("'aria-label', 'placeholder', 'title'"));
        assert!(source.contains("[256, 256, 256, MAX_ATTRIBUTE_BYTES"));
        assert!(source.contains("attr(target, 'aria-labelledby')"));
        assert!(source.contains("contains(joined, 'verification code')"));
        assert!(source.contains("contains(joined, 'access token')"));
        assert!(source.contains("contains(joined, 'card number')"));
        assert!(source.contains("credentialLike(target, isInput) !== false"));
        assert_eq!(source.matches("credentialLike(target, isInput)").count(), 2);
        assert!(source.contains("MAX_METADATA_ID_BYTES = 1024"));
        assert!(source.contains("MAX_LABEL_NODES = 128"));
        assert!(source.contains("MAX_LABELS = 4"));
        assert!(source.contains("beforeinput"));
        assert!(source.contains("insertReplacementText"));
        for forbidden in [
            "querySelector",
            "eval(",
            "new Function",
            "webkit.messageHandlers",
            "fetch(",
            "XMLHttpRequest",
            "WebSocket",
            "localStorage",
            "sessionStorage",
            ".click(",
            ".focus(",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden fill shim surface: {forbidden}"
            );
        }
        assert_eq!(
            source.matches("data-zephium-fill-relay-command-v1").count(),
            1
        );
        assert_eq!(
            source
                .matches("data-zephium-fill-relay-terminal-v1")
                .count(),
            1
        );
        assert_eq!(
            source.matches("data-zephium-fill-relay-ready-v1").count(),
            1
        );
    }
    use zephium_core::ids::ProfileId;

    fn reply() -> ReplyBlock {
        RcBlock::new(|_: *mut AnyObject, _: *mut NSString| {})
    }

    fn invocation(
        invocation: u64,
        generation: SemanticSnapshotGeneration,
    ) -> SemanticRuntimeInvocation {
        let identity = ContextIdentity::new(
            ContextId::generate(),
            ContextRunId::generate(),
            ProfileId::from(91),
            ContextKind::Owned,
        );
        let capabilities =
            ContextCapabilities::try_new(ContextKind::Owned, &[ContextCapability::Observe])
                .expect("capabilities");
        let mut registry = ContextRegistry::new();
        registry.reserve(identity, capabilities).expect("reserve");
        let construction = registry
            .begin_context(
                identity.id(),
                ContextOperationId::new(1).expect("operation"),
            )
            .expect("construction");
        registry
            .settle_construction(identity.id(), construction, ContextSettlement::Applied)
            .expect("settlement");
        let context = registry.join(identity.id()).expect("join");
        let request = SemanticObservationRequest::initial(
            SemanticObservationId::new(1).expect("observation"),
            context,
            SemanticObservationBudget::INITIAL_FILTERED,
        );
        let frame = SemanticFrameJoin::try_new(
            context,
            FrameId::MAIN,
            context.frame_generation(),
            SemanticOrigin::parse("https://semantic.example.test/path").expect("origin"),
            SemanticFrameTrust::SameOrigin,
        )
        .expect("frame");
        encode_semantic_runtime_invocation(
            &request,
            frame,
            SemanticInvocationId::new(invocation).expect("invocation"),
            generation,
            SemanticRuntimeBudget::INITIAL_FILTERED,
        )
        .expect("encode")
    }

    fn success_value(action: ReplyAction) -> Box<str> {
        match action.value {
            ReplyValue::Success(value) => value,
            ReplyValue::Error => panic!("unexpected reply error"),
        }
    }

    #[test]
    fn loading_holds_one_pull_until_exact_commit_and_decodes_one_result() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(7),
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        assert!(matches!(
            state.dispatch_observation(
                invocation(10, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::NotReady, _))
        ));
        let actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply());
        assert!(actions.first_reply.is_none());
        assert!(state.pull.is_some());

        let actions = state.document_committed();
        assert!(!actions.invariant_failed);
        assert!(actions.first_reply.is_none());
        assert_eq!(state.phase, DocumentPhase::Ready);

        let invocation = invocation(11, SemanticSnapshotGeneration::INITIAL);
        let encoded = invocation.as_str().to_owned();
        let actions = state
            .dispatch_observation(invocation, Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("dispatch"));
        assert_eq!(
            success_value(actions.first_reply.expect("request reply")).as_ref(),
            encoded
        );
        assert!(state.awaiting_result);
        assert!(state.pending.is_some());

        let actions = state.on_message("R1:E1:busy", reply());
        assert_eq!(
            success_value(actions.first_reply.expect("acknowledgement")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_ACK
        );
        assert!(matches!(
            actions
                .completion
                .expect("completion")
                .observation_outcome(),
            Err(AgentSemanticRuntimeFailure::Result(
                SemanticRuntimeResultError::Runtime(SemanticRuntimeFault::Busy)
            ))
        ));
        assert_eq!(state.completed_invocations, 1);
        assert!(!state.awaiting_result);
        assert!(state.pending.is_none());
    }

    #[test]
    fn construction_commit_before_view_binding_never_opens_native_dispatch() {
        let mut state = SemanticRuntimeChannelState {
            active_world: Some(8),
            ..SemanticRuntimeChannelState::default()
        };
        let actions = state.document_committed();
        assert!(!actions.invariant_failed);
        assert_eq!(state.phase, DocumentPhase::Ready);
        assert!(matches!(
            state.dispatch_observation(
                invocation(10, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::NotReady, _))
        ));
    }

    #[test]
    fn document_load_renderer_loss_and_retirement_settle_every_retained_owner() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(9),
            active_world: Some(10),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        let invocation = invocation(12, SemanticSnapshotGeneration::INITIAL);
        assert!(state
            .dispatch_observation(invocation, Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("dispatch"))
            .first_reply
            .is_none());
        let actions = state.begin_document_load();
        assert!(matches!(
            actions
                .completion
                .expect("replacement")
                .observation_outcome(),
            Err(AgentSemanticRuntimeFailure::DocumentReplaced)
        ));
        assert_eq!(state.phase, DocumentPhase::Loading);
        assert_eq!(state.active_world, None);
        state.active_world = Some(11);

        assert!(state
            .on_message(SEMANTIC_RUNTIME_CHANNEL_PULL, reply())
            .first_reply
            .is_none());
        assert!(!state.document_committed().invariant_failed);
        let actions = state.renderer_lost();
        assert_eq!(
            success_value(actions.first_reply.expect("stop")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_STOP
        );
        assert_eq!(state.phase, DocumentPhase::RendererLost);
        assert!(state.pending.is_none());
        assert!(state.pull.is_none());

        let actions = state.retire();
        assert!(actions.first_reply.is_none());
        assert_eq!(state.phase, DocumentPhase::Retired);
        assert_eq!(state.active_world, None);
    }

    #[test]
    fn document_invocation_budget_and_audit_invariants_fail_closed() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(13),
            active_world: Some(14),
            phase: DocumentPhase::ExhaustionNoticePending,
            completed_invocations: MAX_SEMANTIC_RUNTIME_DOCUMENT_INVOCATIONS,
            ..SemanticRuntimeChannelState::default()
        };
        assert!(matches!(
            state.dispatch_observation(
                invocation(13, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            ),
            Err((AgentSemanticRuntimeDispatchError::Exhausted, _))
        ));
        assert_eq!(state.phase, DocumentPhase::ExhaustionNoticePending);
        let actions = state.on_message(SEMANTIC_RUNTIME_CHANNEL_EXHAUSTED, reply());
        assert_eq!(
            success_value(actions.first_reply.expect("exhaustion ack")).as_ref(),
            SEMANTIC_RUNTIME_CHANNEL_ACK
        );
        assert_eq!(state.phase, DocumentPhase::Exhausted);

        let mut invalid = SemanticRuntimeChannelState {
            expected_view: Some(17),
            active_world: Some(18),
            phase: DocumentPhase::Ready,
            awaiting_result: true,
            ..SemanticRuntimeChannelState::default()
        };
        let controller = AgentSemanticRuntimeController {
            state: Rc::new(RefCell::new(invalid)),
            on_invariant_failure: Rc::new(|| {}),
            on_callback_panic: Rc::new(|| {}),
        };
        assert_eq!(controller.pending_for_audit(), None);

        invalid = SemanticRuntimeChannelState {
            expected_view: Some(19),
            active_world: Some(20),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        *controller.state.borrow_mut() = invalid;
        assert_eq!(controller.pending_for_audit(), Some(false));

        controller.state.borrow_mut().active_world = None;
        assert_eq!(controller.pending_for_audit(), None);
    }

    #[test]
    fn timeout_is_invocation_exact_and_permanently_stops_the_document_channel() {
        let mut state = SemanticRuntimeChannelState {
            expected_view: Some(23),
            active_world: Some(24),
            phase: DocumentPhase::Ready,
            ..SemanticRuntimeChannelState::default()
        };
        let invocation_id = SemanticInvocationId::new(22).expect("invocation");
        state
            .dispatch_observation(
                invocation(22, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .unwrap_or_else(|_| panic!("dispatch"));
        let (actions, matched) =
            state.timeout(SemanticInvocationId::new(21).expect("different invocation"));
        assert!(!matched);
        assert!(actions.completion.is_none());
        assert!(state.pending.is_some());

        let (actions, matched) = state.timeout(invocation_id);
        assert!(matched);
        assert!(matches!(
            actions
                .completion
                .expect("timeout completion")
                .observation_outcome(),
            Err(AgentSemanticRuntimeFailure::TimedOut)
        ));
        assert_eq!(state.phase, DocumentPhase::Failed);
        assert!(state.pending.is_none());
        assert!(state
            .dispatch_observation(
                invocation(23, SemanticSnapshotGeneration::INITIAL),
                Box::new(|_| {}),
            )
            .is_err());
    }
}
