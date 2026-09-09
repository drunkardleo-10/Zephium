// Follow-up: fixed editable-ancestor fencing experiment. Not linked to Zephium or production admission.
import AppKit
import WebKit

final class Probe: NSObject, WKScriptMessageHandler, WKUIDelegate, WKNavigationDelegate {
    var view: WKWebView!
    var window: NSWindow!
    var popupCount = 0
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { print("navigation-finished") }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { print("navigation-failed:\(error)") }
    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        if let body = message.body as? String { print(body); if body == "started" || body.hasPrefix("script-error:") { return } }
        print("popupCount=\(popupCount)")
        NSApplication.shared.terminate(nil)
    }
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration, for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        popupCount += 1
        return nil
    }
    func run() {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        config.preferences.javaScriptCanOpenWindowsAutomatically = false
        if #available(macOS 14.0, *) { config.preferences.inactiveSchedulingPolicy = .none }
        let world = WKContentWorld.world(name: "fixed-edit-command-probe")
        config.userContentController.add(self, contentWorld: world, name: "result")
        let script = #"""
        (() => {
          'use strict';
          window.webkit.messageHandlers.result.postMessage('started');
          window.addEventListener('error',e=>window.webkit.messageHandlers.result.postMessage('script-error:'+e.message));
          const exec = Document.prototype.execCommand;
          const select = window.getSelection.bind(window);
          const focus = HTMLElement.prototype.focus;
          const results = [];
          function run() {
            for (const kind of ['nested','fenced-normal','fenced-reopen-beforeinput','fenced-observer-revert']) {
              const host = document.getElementById(kind);
              const leaf = host.querySelector('[data-leaf]');
              const other = host.querySelector('[data-other]');
              const sentinel = document.getElementById('sentinel');
              focus.call(sentinel);
              const oldFocus = document.activeElement;
              const selection = select();
              const oldRanges = Array.from({length:selection.rangeCount},(_,i)=>selection.getRangeAt(i).cloneRange());
              const beforeOther = other.innerHTML;
              const ancestor = leaf.parentElement;
              const fenced = kind.startsWith('fenced-');
              const originalAttribute = ancestor.getAttribute('contenteditable');
              if (fenced) ancestor.setAttribute('contenteditable','false');
              const sibling = host.querySelector('[data-sibling]');
              const beforeSibling = sibling ? sibling.innerHTML : null;
              focus.call(leaf);
              const range = document.createRange(); range.selectNodeContents(leaf);
              selection.removeAllRanges(); selection.addRange(range);
              const valid = leaf.isConnected && range.commonAncestorContainer === leaf && leaf.textContent === 'Original';
              let returned = null, error = null;
              if (valid) { try { returned = exec.call(document,kind === 'unsupported' ? 'zephiumInvalidCommand' : 'insertText',false,'Bounded text'); } catch(e) { error = e.name; } }
              if (fenced) { if(originalAttribute === null) ancestor.removeAttribute('contenteditable'); else ancestor.setAttribute('contenteditable',originalAttribute); }
              selection.removeAllRanges(); for (const r of oldRanges) selection.addRange(r);
              focus.call(oldFocus);
              results.push({kind,valid,returned,error,ancestorAttributeRestored:ancestor.getAttribute('contenteditable')===originalAttribute,leaf:leaf.textContent,connected:leaf.isConnected,markup:host.innerHTML,other:other.textContent,otherPreserved:other.innerHTML===beforeOther,siblingPreserved:!sibling||(sibling.isConnected&&sibling.innerHTML===beforeSibling),focusRestored:document.activeElement===oldFocus,activation:navigator.userActivation.isActive,events:host.getAttribute('data-events'),model:host.getAttribute('data-model')});
            }
            setTimeout(()=>window.webkit.messageHandlers.result.postMessage(JSON.stringify({engine:navigator.userAgent,results,retained:Array.from(document.querySelectorAll('[data-model]')).map(h=>({kind:h.id,model:h.getAttribute('data-model'),observer:h.getAttribute('data-observer'),ancestorAttribute:h.firstElementChild.getAttribute('contenteditable'),value:h.querySelector('[data-leaf]')?.textContent??null}))})),0);
          }
          document.addEventListener('DOMContentLoaded',()=>{try{run()}catch(e){window.webkit.messageHandlers.result.postMessage('error:'+e.name+':'+e.message)}},{once:true});
        })();
        """#
        config.userContentController.addUserScript(WKUserScript(source: script, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: config)
        view.uiDelegate = self
        view.navigationDelegate = self
        window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 800, height: 600), styleMask: .borderless, backing: .buffered, defer: false)
        window.contentView = view
        let html = #"""
        <!doctype html><meta charset="utf-8"><body><input id="sentinel"><script>
        for (const kind of ['nested','fenced-normal','fenced-reopen-beforeinput','fenced-observer-revert']) {
          const host=document.createElement('section'); host.id=kind;
          host.innerHTML = (kind==='nested'||kind.startsWith('fenced-')) ? '<div contenteditable="true"><span data-leaf contenteditable="true">Original</span><b data-sibling>Sibling</b></div><div data-other contenteditable="true">Other</div>' : '<div data-leaf contenteditable="true">'+(kind==='rich'?'<b>Original</b>':'Original')+'</div><div data-other contenteditable="true">Other</div>';
          document.body.append(host);
          const leaf=host.querySelector('[data-leaf]'), other=host.querySelector('[data-other]');
          let events=[], model='Original', reentered=false;
          host.setAttribute('data-model',model);
          if(kind.startsWith('fenced-'))new MutationObserver(records=>{host.setAttribute('data-observer',JSON.stringify(records.map(r=>({old:r.oldValue,current:r.target.getAttribute('contenteditable')}))));if(kind==='fenced-observer-revert'){model='Original';host.setAttribute('data-model',model);if(leaf.isConnected)leaf.textContent=model;}}).observe(leaf.parentElement,{attributes:true,attributeFilter:['contenteditable'],attributeOldValue:true});
          function retarget(){other.focus();const r=document.createRange();r.selectNodeContents(other);const s=getSelection();s.removeAllRanges();s.addRange(r);}
          for (const type of ['beforeinput','input','textInput']) host.addEventListener(type,e=>{
            events.push({type:e.type,trusted:e.isTrusted,inputType:e.inputType||null,data:e.data,target:e.target===other?'other':e.target===leaf?'leaf':'ancestor',cancelable:e.cancelable,activation:navigator.userActivation.isActive});
            host.setAttribute('data-events',JSON.stringify(events));
            if(kind==='fenced-reopen-beforeinput' && type==='beforeinput')leaf.parentElement.setAttribute('contenteditable','true');
            if(kind==='cancel-beforeinput' && type==='beforeinput')e.preventDefault();
            if(kind==='retarget-beforeinput' && type==='beforeinput')retarget();
            if(kind==='replace-beforeinput' && type==='beforeinput'){const replacement=leaf.cloneNode(true);leaf.replaceWith(replacement);replacement.focus();const r=document.createRange();r.selectNodeContents(replacement);getSelection().removeAllRanges();getSelection().addRange(r);}
            if(kind==='reentrant-beforeinput' && type==='beforeinput' && !reentered){reentered=true;retarget();document.execCommand('insertText',false,'Reentrant');}
            if(kind==='retarget-textInput' && type==='textInput')retarget();
            if(kind==='cancel-textInput' && type==='textInput')e.preventDefault();
            if(kind==='replace-textInput' && type==='textInput'){const replacement=leaf.cloneNode(true);leaf.replaceWith(replacement);replacement.focus();const r=document.createRange();r.selectNodeContents(replacement);getSelection().removeAllRanges();getSelection().addRange(r);}
            if(kind==='reentrant-textInput' && type==='textInput' && !reentered){reentered=true;retarget();document.execCommand('insertText',false,'Reentrant');}
            if(type==='input' && e.isTrusted){model=leaf.textContent;host.setAttribute('data-model',model);queueMicrotask(()=>{if(leaf.isConnected)leaf.textContent=model;});}
            if(type==='input')window.open('about:blank','_blank');
          });
        }
        </script>
        """#
        view.loadHTMLString(html, baseURL: URL(string: "https://fixed.invalid/"))
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.finishLaunching()
let probe = Probe()
probe.run()
DispatchQueue.main.asyncAfter(deadline: .now()+20) { print("timeout"); app.terminate(nil) }
app.run()
