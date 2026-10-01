import Cocoa
import WebKit

// A WKWebView host on the system WebKit, as the app runs it (non-persistent data store).
// Commands arrive on stdin, one per line; each answers with one line of JSON.
final class Host: NSObject, NSApplicationDelegate {
  var window: NSWindow!
  var web: WKWebView!

  func applicationDidFinishLaunching(_ note: Notification) {
    let config = WKWebViewConfiguration()
    config.websiteDataStore = WKWebsiteDataStore.nonPersistent()
    web = WKWebView(frame: NSRect(x: 0, y: 0, width: 1440, height: 900), configuration: config)
    window = NSWindow(
      contentRect: NSRect(x: 0, y: 0, width: 1440, height: 900),
      styleMask: [.titled], backing: .buffered, defer: false)
    window.contentView = web
    window.makeKeyAndOrderFront(nil)
    let stdin = FileHandle.standardInput
    DispatchQueue.global().async {
      while let line = readLine(strippingNewline: true) { DispatchQueue.main.async { self.run(line) } }
      _ = stdin
      exit(0)
    }
    reply(["ready": true])
  }

  func reply(_ value: Any) {
    let data = try! JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])
    print(String(data: data, encoding: .utf8)!)
    fflush(stdout)
  }

  func run(_ line: String) {
    let parts = line.split(separator: " ", maxSplits: 1).map(String.init)
    let arg = parts.count > 1 ? parts[1] : ""
    switch parts[0] {
    case "load":
      web.load(URLRequest(url: URL(string: arg)!))
      waitLoaded()
    case "eval":
      web.callAsyncJavaScript(arg, arguments: [:], in: nil, in: .page) { result in
        switch result {
        case .success(let value): self.reply(["ok": value ?? NSNull()])
        case .failure(let error): self.reply(["error": "\(error)"])
        }
      }
    case "pid":
      reply(["pid": web.value(forKey: "_webProcessIdentifier") ?? 0])
    case "memory-cache":
      let types: Set<String> = [WKWebsiteDataTypeMemoryCache]
      web.configuration.websiteDataStore.removeData(ofTypes: types, modifiedSince: .distantPast) {
        self.reply(["cleared": "memory-cache"])
      }
    case "call":
      let selector = Selector(arg)
      if web.responds(to: selector) {
        _ = web.perform(selector)
        reply(["called": arg])
      } else if let pool = Optional(web.configuration.processPool), pool.responds(to: selector) {
        _ = pool.perform(selector)
        reply(["called": "pool " + arg])
      } else {
        reply(["missing": arg])
      }
    case "methods":
      var names: [String] = []
      for name in arg.split(separator: " ") {
        guard let cls = NSClassFromString(String(name)) else { continue }
        for target in [cls, object_getClass(cls)!] {
          var count: UInt32 = 0
          let list = class_copyMethodList(target, &count)
          for index in 0..<Int(count) {
            let selector = NSStringFromSelector(method_getName(list![index]))
            if selector.range(of: "arbage|emory|ache|elease|urge|scaveng|ressure|Purge", options: .regularExpression) != nil {
              names.append("\(name).\(selector)")
            }
          }
          free(list)
        }
      }
      reply(names)
    case "hide":
      window.miniaturize(nil)
      reply(["hidden": true])
    case "show":
      window.deminiaturize(nil)
      window.makeKeyAndOrderFront(nil)
      reply(["shown": true])
    case "quit":
      exit(0)
    default:
      reply(["error": "unknown"])
    }
  }

  func waitLoaded() {
    if web.isLoading {
      DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { self.waitLoaded() }
    } else {
      reply(["loaded": web.url?.absoluteString ?? ""])
    }
  }
}

let app = NSApplication.shared
let host = Host()
app.delegate = host
app.setActivationPolicy(.regular)
app.run()
