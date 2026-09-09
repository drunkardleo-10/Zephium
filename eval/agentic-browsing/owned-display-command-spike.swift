// Fixed local mechanism proof, release-excluded. No evaluation API, native input,
// accounts, credentials, private selectors, capture data, or production admission.
import AppKit
import WebKit

final class DisplayCommandProbe: NSObject, WKUIDelegate, WKScriptMessageHandler {
    var view: WKWebView!
    var window: NSWindow!
    var timer: Timer?
    var initialWindows = Set<Int>()
    var dispatched = false
    var samples = 0
    var finished = false

    func webView(_ webView: WKWebView, requestMediaCapturePermissionFor origin: WKSecurityOrigin,
                 initiatedByFrame frame: WKFrameInfo, type: WKMediaCaptureType,
                 decisionHandler: @escaping (WKPermissionDecision) -> Void) {
        decisionHandler(.deny)
    }
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? { nil }
    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let result = message.body as? String, result.utf8.count < 1024 else { finish("invalid-witness"); return }
        print(result)
        if result.hasPrefix("before-command ") {
            initialWindows = Set(NSApplication.shared.windows.map { $0.windowNumber })
            print("dispatch-baseline active=\(NSApplication.shared.isActive)")
            dispatched = true
        }
        if result.hasPrefix("settled ") { finish("settled") }
    }
    func finish(_ reason: String) {
        timer?.invalidate()
        print("native-end reason=\(reason) samples=\(samples) production-admission=absent")
        view.stopLoading()
        window.orderOut(nil)
        finished = true
    }
    func run() throws {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        if #available(macOS 12.3, *) { configuration.preferences.isElementFullscreenEnabled = false }
        if #available(macOS 14.0, *) { configuration.preferences.inactiveSchedulingPolicy = .throttle }
        let world = WKContentWorld.world(name: "fixed-document-start-display-command")
        configuration.userContentController.add(self, contentWorld: world, name: "result")
        let script = #"""
        (() => {
          'use strict';
          const command = Document.prototype.execCommand;
          const report = message => window.webkit.messageHandlers.result.postMessage(message);
          document.addEventListener('DOMContentLoaded', () => {
            setTimeout(() => {
              const button = document.getElementById('prepare');
              button.click();
              report(`before-command secure=${isSecureContext} hidden=${document.hidden} active=${navigator.userActivation.isActive} sticky=${navigator.userActivation.hasBeenActive}`);
              const result = command.call(document, 'insertText', false, 'local surface witness');
              report(`after-command returned=${result} ` + document.getElementById('witness').textContent);
              setTimeout(() => report('settled ' + document.getElementById('witness').textContent), 3500);
            }, 250);
          }, { once: true });
        })();
        """#
        configuration.userContentController.addUserScript(WKUserScript(source: script,
            injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 600), configuration: configuration)
        view.uiDelegate = self
        window = NSWindow(contentRect: view.frame, styleMask: .borderless, backing: .buffered, defer: false)
        window.contentView = view
        window.ignoresMouseEvents = true
        window.orderFrontRegardless()
        initialWindows = Set(NSApplication.shared.windows.map { $0.windowNumber })
        let fixture = try String(contentsOfFile: "crates/zephium-agentic/assets/owned-surface-probe-v1.html", encoding: .utf8)
        var html = fixture.replacingOccurrences(of: "const selected = location.hash.slice(1);", with: "const selected = 'display';")
        let control = CommandLine.arguments.dropFirst().elementsEqual(["--control"])
        if control {
            // Fixed negative control still inserts text and dispatches trusted
            // input; only this fixture's display operation is replaced.
            html = html.replacingOccurrences(of: "navigator.mediaDevices.getDisplayMedia({video:true})",
                with: "Promise.reject({name:'NotAllowedError'})")
        }
        print("baseline active=\(NSApplication.shared.isActive) control=\(control)")
        view.loadHTMLString(html, baseURL: URL(string: "http://localhost/"))
        timer = Timer.scheduledTimer(withTimeInterval: 0.005, repeats: true) { [weak self] _ in
            guard let self, self.dispatched else { return }
            self.samples += 1
            let app = NSApplication.shared
            let newWindow = app.windows.contains { !self.initialWindows.contains($0.windowNumber) }
            let sheet = app.windows.contains { $0.attachedSheet != nil }
            if app.isActive || newWindow || sheet || app.modalWindow != nil {
                self.finish("dispatched=\(self.dispatched),active=\(app.isActive),new-window=\(newWindow),sheet=\(sheet),modal=\(app.modalWindow != nil)")
            }
        }
    }
}
let app = NSApplication.shared
guard !app.isActive, app.windows.isEmpty else { print("launch-existing-authority"); exit(2) }
if app.activationPolicy() != .prohibited && !app.setActivationPolicy(.prohibited) { print("launch-prohibition-failed"); exit(2) }
app.finishLaunching()
guard !app.isActive, app.windows.isEmpty else { print("launch-activated"); exit(2) }
if app.activationPolicy() != .accessory && !app.setActivationPolicy(.accessory) { print("launch-accessory-failed"); exit(2) }
guard !app.isActive else { print("launch-accessory-activated"); exit(2) }
let probe = DisplayCommandProbe()
do { try probe.run() } catch { print("fixture-read-failed"); exit(1) }
DispatchQueue.main.asyncAfter(deadline: .now() + 12) { probe.finish("timeout") }
// NSApplication.run starts its own launch/activation handling. Match the owned
// qualifier's explicit run-loop pump instead, without dispatching human input.
while !probe.finished {
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.005))
    for _ in 0..<32 {
        guard let event = app.nextEvent(matching: .appKitDefined, until: .distantPast,
                                       inMode: .default, dequeue: true) else { break }
        app.sendEvent(event)
    }
}
