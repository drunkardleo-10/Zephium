// Compiled only into the selected native-agentic-semantic-probe process.
// Fixed document-start isolated-world command; no evaluation/native gesture.
(() => {
  'use strict';
  if (location.hostname !== 'localhost' ||
      location.pathname !== '/owned-surface-probe-v1.html' ||
      location.hash !== '#display-document-start') return;
  const command = Document.prototype.execCommand;
  let samples = 0;
  const timer = setInterval(() => {
    if (++samples > 1200) { clearInterval(timer); return; }
    const setup = document.getElementById('setup');
    if (document.hidden || setup?.textContent !== 'Surface setup prepared') return;
    clearInterval(timer);
    // Allow the native owner to finish its prepared semantic snapshot and
    // attest its exact view, first responder, store and navigation joins.
    setTimeout(() => {
      const editor = document.getElementById('editor');
      if (document.hidden || document.activeElement !== editor ||
          editor?.value !== 'original') return;
      const witness = document.createElement('h3');
      witness.textContent = `Command witness hidden ${document.hidden} active ${navigator.userActivation.isActive} sticky ${navigator.userActivation.hasBeenActive}`;
      document.querySelector('main').append(witness);
      command.call(document, 'insertText', false, 'local surface witness');
    }, 500);
  }, 10);
})();
