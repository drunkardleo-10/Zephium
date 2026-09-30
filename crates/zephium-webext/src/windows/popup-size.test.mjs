import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {test} from 'node:test';
import vm from 'node:vm';

const source = readFileSync(new URL('./popup-size.js', import.meta.url), 'utf8');
function fixture({frame = false, reduced = false} = {}) {
  const messages = [], events = {}, frames = [];
  const state = {width: 320, height: 80, rootWidth: 400, rootHeight: 600,
    animations: 0, disconnected: false, observer: null};
  const window = {};
  const document = {readyState: 'loading',
    documentElement: {get scrollWidth() { return state.rootWidth; },
      get scrollHeight() { return state.rootHeight; }, animate() { state.animations++; }},
    body: {get scrollWidth() { return state.width; }, get scrollHeight() { return state.height; },
      getBoundingClientRect: () => ({width: state.width, height: state.height})}};
  vm.runInNewContext(source, {window, top: frame ? {} : window, document,
    innerWidth: 400, innerHeight: 600,
    getComputedStyle: () => ({marginLeft: '0', marginRight: '0', marginTop: '0', marginBottom: '0'}),
    matchMedia: () => ({matches: reduced}),
    chrome: {webview: {postMessage: value => messages.push(JSON.parse(value))}},
    addEventListener: (name, handler) => { events[name] = handler; },
    requestAnimationFrame: handler => frames.push(handler),
    ResizeObserver: class { constructor(callback) { state.observer = callback; }
      observe() {} disconnect() { state.disconnected = true; } }
  });
  const resize = () => { state.observer(); state.observer(); assert.equal(frames.length, 1); frames.shift()(); };
  return {state, window, messages, events, resize};
}

test('first size waits for load but does not depend on a hidden-window animation frame', () => {
  const {messages, events} = fixture();
  assert.equal(messages.length, 0);
  events.load();
  assert.deepEqual(messages, [{kind: 'popup-size', width: 320, height: 80}]);
});

test('content can grow and shrink; observer bursts coalesce and unchanged sizes do not repeat', () => {
  const {messages, events, state, resize} = fixture();
  events.load();
  resize();
  assert.equal(messages.length, 1);
  state.height = 500;
  resize();
  assert.equal(messages.at(-1).height, 500);
  state.height = 60;
  resize();
  assert.equal(messages.at(-1).height, 60);
  events.pagehide();
  assert.equal(state.disconnected, true);
});

test('size is bounded and overflowing root content is included', () => {
  const {messages, events, state, resize} = fixture();
  state.width = 2000; state.height = 3000;
  events.load();
  assert.deepEqual(messages.at(-1), {kind: 'popup-size', width: 800, height: 600});
  state.width = 0; state.height = 0;
  resize();
  assert.deepEqual(messages.at(-1), {kind: 'popup-size', width: 25, height: 25});
  state.rootWidth = 650;
  resize();
  assert.equal(messages.at(-1).width, 650);
});

test('animation respects reduced motion and frames cannot request sizing', () => {
  for (const reduced of [false, true]) {
    const {window, state} = fixture({reduced});
    window.__zephiumPopupShown();
    assert.equal(state.animations, reduced ? 0 : 1);
  }
  const {window, events} = fixture({frame: true});
  assert.equal(window.__zephiumPopupShown, undefined);
  assert.deepEqual(events, {});
});
