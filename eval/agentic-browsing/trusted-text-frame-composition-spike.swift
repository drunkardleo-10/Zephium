// Fixed local public-API counterexample probe. Never linked to the product.
// No provider, network request, account, global input, clipboard, or eval API.
import AppKit
import WebKit

final class TrustedTextProbe: NSObject, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
    let cases = ["main", "dynamic-blank-denied", "sandbox-srcdoc", "sandbox-srcdoc-denied", "composition"]
    var index = 0
    var view: WKWebView!
    var window: NSWindow!
    var navigations: [[String: Any]] = []
    var events: [[String: Any]] = []
    var entered = false
    var markedBefore = false
    var results: [[String: Any]] = []
    var current: String { cases[index] }

    func start() {
        navigations = []; events = []; entered = false; markedBefore = false
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        if #available(macOS 14.0, *) { configuration.preferences.inactiveSchedulingPolicy = .none }
        let world = WKContentWorld.world(name: "trusted-text-frame-composition-proof")
        configuration.userContentController.add(self, contentWorld: world, name: "witness")
        // This isolated observer never dispatches native input. A host timer owns
        // the sole dispatch; page messages are bounded diagnostic evidence only.
        let observer = #"""
        (() => {
          'use strict';
          let count = 0;
          const send = data => { if (++count <= 24) window.webkit.messageHandlers.witness.postMessage(data); };
          for (const type of ['focusin','beforeinput','textInput','input','compositionstart','compositionupdate','compositionend']) {
            document.addEventListener(type, e => send({type, target:e.target.id || e.target.tagName,
              trusted:e.isTrusted, inputType:e.inputType || '', composing:e.isComposing || false,
              value:e.target.value || e.target.textContent || '', active:navigator.userActivation.isActive,
              sticky:navigator.userActivation.hasBeenActive}), true);
          }
          window.addEventListener('error',e=>send({type:'error',message:e.message}));
          setTimeout(() => send({type:'settled', value:document.getElementById('editor')?.value || '',
            mainValue:document.getElementById('main')?.value || '', origin:origin,
            scriptAttempt:document.body?.getAttribute('data-attempt') || '',
            activeElement:document.activeElement?.id || document.activeElement?.tagName || ''}), 2400);
        })();
        """#
        configuration.userContentController.addUserScript(WKUserScript(source: observer,
            injectionTime: .atDocumentStart, forMainFrameOnly: false, in: world))
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: configuration)
        for name in ["insertText:replacementRange:", "hasMarkedText", "setMarkedText:selectedRange:replacementRange:"] {
            guard view.responds(to: NSSelectorFromString(name)) else { fail("unsupported public selector") }
        }
        view.navigationDelegate = self; view.uiDelegate = self
        window = NSWindow(contentRect: view.frame, styleMask: .borderless, backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = view
        window.ignoresMouseEvents = true
        precondition(window.makeFirstResponder(view))
        let child = #"<input id='editor' value='child original'><script>setTimeout(()=>{document.body.setAttribute('data-attempt','ran');const e=document.getElementById('editor');e.focus();e.select();document.body.setAttribute('data-attempt','done');},600)</script>"#
        let setup: String
        switch current {
        case "dynamic-blank-denied":
            setup = #"setTimeout(()=>{const f=document.createElement('iframe');document.body.append(f);const d=f.contentDocument;d.body.innerHTML='<input id=editor value="child original">';const e=d.getElementById('editor');e.focus();e.select();},200);"#
        case "sandbox-srcdoc", "sandbox-srcdoc-denied":
            let encoded = String(data: try! JSONSerialization.data(withJSONObject: [child]), encoding: .utf8)!
            setup = "setTimeout(()=>{const f=document.createElement('iframe');f.sandbox='allow-scripts';f.srcdoc=(\(encoded))[0];document.body.append(f);setTimeout(()=>f.focus(),400);},100);"
        default: setup = ""
        }
        let html = "<!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content=\"default-src 'none'; script-src 'unsafe-inline'; frame-src about:; style-src 'unsafe-inline'\"><body><input id=main value='main original'><script>main.focus();main.select();\(setup)</script>"
        view.loadHTMLString(html, baseURL: URL(string: "https://fixed.invalid/"))
        let expectedIndex = index
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [self] in
            guard index == expectedIndex else { return }
            if current == "composition" {
                (view as AnyObject).setMarkedText!("marked seed", selectedRange: NSRange(location: 6, length: 4), replacementRange: NSRange(location: NSNotFound, length: 0))
            }
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.4) { [self] in
            guard index == expectedIndex, !entered else { return }
            markedBefore = (view as AnyObject).hasMarkedText!()
            entered = true
            (view as AnyObject).insertText!("bounded insertion", replacementRange: NSRange(location: NSNotFound, length: 0))
        }
        DispatchQueue.main.asyncAfter(deadline: .now() + 3.2) { [self] in
            guard index == expectedIndex else { return }
            finish()
        }
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.webView === view, events.count < 96,
              let data = message.body as? [String: Any],
              let type = data["type"] as? String,
              ["focusin", "beforeinput", "textInput", "input", "compositionstart", "compositionupdate", "compositionend", "settled", "error"].contains(type),
              JSONSerialization.isValidJSONObject(data),
              let encoded = try? JSONSerialization.data(withJSONObject: data), encoded.count < 2048 else { return }
        let origin = message.frameInfo.securityOrigin
        events.append(["witness": data, "mainFrame": message.frameInfo.isMainFrame,
            "phase": entered ? "after-insert" : "before-insert",
            "nativeOrigin": "\(origin.protocol)://\(origin.host):\(origin.port)",
            "nativeFrameURL": message.frameInfo.request.url?.absoluteString ?? "absent"])
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let target = navigationAction.request.url?.absoluteString ?? "absent"
        let main = navigationAction.targetFrame?.isMainFrame ?? false
        let allow = navigations.isEmpty && main && target == "https://fixed.invalid/"
            || current == "sandbox-srcdoc" && !main && target == "about:srcdoc"
        if navigations.count < 24 { navigations.append(["url": target, "mainFrame": main, "allowed": allow]) }
        decisionHandler(allow ? .allow : .cancel)
    }

    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }

    func finish() {
        validateRow()
        results.append(["case": current, "entered": entered, "nativeURL": view.url?.absoluteString ?? "absent",
            "markedBeforeInsert": markedBefore, "markedAfterInsert": (view as AnyObject).hasMarkedText!(),
            "appActive": NSApplication.shared.isActive, "windowVisible": window.isVisible,
            "windowKey": window.isKeyWindow, "windowMain": window.isMainWindow,
            "nativeResponderExact": window.firstResponder === view, "navigations": navigations, "events": events])
        view.configuration.userContentController.removeScriptMessageHandler(forName: "witness", contentWorld: WKContentWorld.world(name: "trusted-text-frame-composition-proof"))
        view.stopLoading(); window.contentView = nil; window.close(); view = nil; window = nil
        index += 1
        if index < cases.count { start(); return }
        let data = try! JSONSerialization.data(withJSONObject: ["results": results], options: [.prettyPrinted, .sortedKeys])
        print(String(data: data, encoding: .utf8)!)
        NSApplication.shared.terminate(nil)
    }

    func validateRow() {
        guard entered, view.url?.absoluteString == "https://fixed.invalid/",
              window.firstResponder === view, !window.isVisible, !window.isKeyWindow,
              !window.isMainWindow, !NSApplication.shared.isActive,
              !events.contains(where: { ($0["witness"] as? [String: Any])?["type"] as? String == "error" })
        else { fail("native ownership, lifecycle, or fixture script") }
        let inputs = events.filter { ($0["witness"] as? [String: Any])?["type"] as? String == "input" }
        let mainSettled = events.first {
            $0["mainFrame"] as? Bool == true && ($0["witness"] as? [String: Any])?["type"] as? String == "settled"
        }?["witness"] as? [String: Any]
        let expectedMain = ["main", "composition"].contains(current) ? "bounded insertion" : "main original"
        guard mainSettled?["mainValue"] as? String == expectedMain else { fail("main settled value") }
        guard inputs.allSatisfy({
            let data = $0["witness"] as? [String: Any]
            return data?["trusted"] as? Bool == true && data?["active"] as? Bool == false && data?["sticky"] as? Bool == false
        }) else { fail("event trust or activation") }
        switch current {
        case "main":
            guard inputs.count == 1, inputs[0]["mainFrame"] as? Bool == true else { fail("main input") }
        case "dynamic-blank-denied":
            guard inputs.count == 1, inputs[0]["mainFrame"] as? Bool == false,
                  inputs[0]["nativeOrigin"] as? String == "https://fixed.invalid:0",
                  (inputs[0]["witness"] as? [String: Any])?["value"] as? String == "bounded insertion",
                  navigations.contains(where: { $0["url"] as? String == "about:blank" && $0["allowed"] as? Bool == false })
            else { fail("denied navigation initial-document counterexample") }
        case "sandbox-srcdoc":
            guard inputs.isEmpty, events.contains(where: {
                let data = $0["witness"] as? [String: Any]
                return $0["mainFrame"] as? Bool == false && $0["nativeOrigin"] as? String == "://:0"
                    && data?["origin"] as? String == "null" && data?["scriptAttempt"] as? String == "done"
                    && data?["value"] as? String == "child original"
            }) else { fail("opaque frame observation") }
        case "sandbox-srcdoc-denied":
            guard inputs.isEmpty,
                  navigations.contains(where: { $0["url"] as? String == "about:srcdoc" && $0["allowed"] as? Bool == false })
            else { fail("denied sandbox frame") }
        case "composition":
            let types = inputs.compactMap { ($0["witness"] as? [String: Any])?["inputType"] as? String }
            guard !markedBefore, types == ["deleteByComposition", "insertCompositionText", "deleteCompositionText", "insertFromComposition"],
                  inputs[1]["phase"] as? String == "before-insert",
                  inputs[2]["phase"] as? String == "after-insert",
                  events.contains(where: { ($0["witness"] as? [String: Any])?["type"] as? String == "compositionend" })
            else { fail("synchronous marked-text getter counterexample") }
        default: fail("unknown case")
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
app.finishLaunching()
let probe = TrustedTextProbe()
probe.start()
DispatchQueue.main.asyncAfter(deadline: .now() + 25) { fputs("probe timed out\n", stderr); exit(1) }
app.run()

func fail(_ reason: String) -> Never {
    fputs("probe failed: \(reason)\n", stderr)
    exit(1)
}
