// Fixed local logical-editor proof; never linked to production admission.
import AppKit
import WebKit

final class DocumentCommandProbe: NSObject, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
    var view: WKWebView!
    var window: NSWindow!
    var events: [[String: Any]] = []
    var navigations: [[String: Any]] = []
    var popupCount = 0
    func start() {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        if #available(macOS 14.0, *) { configuration.preferences.inactiveSchedulingPolicy = .none }
        let world = WKContentWorld.world(name: "trusted-document-command-proof")
        configuration.userContentController.add(self, contentWorld: world, name: "witness")
        let observer = #"""
        (() => {
          'use strict';
          const send = window.webkit.messageHandlers.witness.postMessage.bind(window.webkit.messageHandlers.witness);
          let count = 0;
          for (const type of ['beforeinput','input','textInput']) document.addEventListener(type,e=>{
            if (++count <= 160) send({kind:'event',type,target:e.target.id || e.target.tagName,
              row:e.target.closest('section')?.id || document.body?.getAttribute('data-case') || '',
              trusted:e.isTrusted,inputType:e.inputType || '',data:e.data || '',
              active:navigator.userActivation.isActive,sticky:navigator.userActivation.hasBeenActive});
          },true);
          window.addEventListener('error',e=>send({kind:'error',message:e.message}));
        })();
        """#
        let command = #"""
        (() => {
          'use strict';
          const exec = Document.prototype.execCommand;
          const focus = HTMLElement.prototype.focus;
          const rangeSelect = Range.prototype.selectNodeContents;
          const send = window.webkit.messageHandlers.witness.postMessage.bind(window.webkit.messageHandlers.witness);
          const cases = ['normal','same-document-beforeinput','cross-frame-beforeinput','cross-frame-textInput',
            'cross-frame-before-command','reentrant-cross-frame','adopt-leaf-cross-frame','cancel','replace-leaf'];
          const results = [];
          document.addEventListener('DOMContentLoaded',()=>setTimeout(next,350),{once:true});
          function next() {
            const kind = cases[results.length];
            if (!kind) { send({kind:'complete',results}); return; }
            const section = document.getElementById(kind);
            const unit = section.querySelector('[data-unit]'), leaf = section.querySelector('[data-leaf]');
            const sibling = section.querySelector('[data-sibling]'), decoy = section.querySelector('[data-decoy]');
            const child = section.querySelector('iframe'), childDocument = child.contentDocument;
            const childEditor = childDocument.getElementById('child-editor');
            const oldSibling = sibling.outerHTML;
            focus.call(leaf);
            const range = document.createRange(); rangeSelect.call(range,leaf);
            const selection = getSelection(); selection.removeAllRanges(); selection.addRange(range);
            const valid = leaf.isConnected && leaf.ownerDocument === document && unit.contains(leaf)
              && range.commonAncestorContainer === leaf && leaf.textContent === 'unit original';
            if (kind === 'cross-frame-before-command') { childEditor.focus(); childEditor.select(); }
            let returned = null, error = null;
            if (valid) { try { returned = exec.call(document,'insertText',false,'document replacement'); } catch(e) { error=e.name; } }
            const immediate = {leafConnected:leaf.isConnected,leafDocumentMain:leaf.ownerDocument===document,
              unit:unit.textContent,child:childEditor.value,decoy:decoy.value};
            setTimeout(()=>{
              const fresh = unit.querySelector('[data-leaf]');
              results.push({kind,valid,returned,error,immediate,
                model:unit.getAttribute('data-model'),freshValue:fresh?.textContent ?? null,
                rerender:Number(unit.getAttribute('data-rerenders')),leafReplaced:fresh!==leaf,
                siblingIntact:sibling.isConnected&&sibling.parentNode===unit&&sibling.outerHTML===oldSibling,
                decoy:decoy.value,child:childEditor.value,originalLeafValue:leaf.textContent,
                originalLeafDocumentMain:leaf.ownerDocument===document,
                mainDocumentStillExact:document===unit.ownerDocument&&unit.isConnected,
                active:navigator.userActivation.isActive,sticky:navigator.userActivation.hasBeenActive,
                handlerCounts:JSON.parse(section.getAttribute('data-counts'))});
              next();
            },100);
          }
        })();
        """#
        configuration.userContentController.addUserScript(WKUserScript(source: observer, injectionTime: .atDocumentStart, forMainFrameOnly: false, in: world))
        configuration.userContentController.addUserScript(WKUserScript(source: command, injectionTime: .atDocumentStart, forMainFrameOnly: true, in: world))
        view = WKWebView(frame: NSRect(x:0,y:0,width:1000,height:800),configuration:configuration)
        view.navigationDelegate = self; view.uiDelegate = self
        window = NSWindow(contentRect:view.frame,styleMask:.borderless,backing:.buffered,defer:false)
        window.isReleasedWhenClosed = false; window.contentView = view; window.ignoresMouseEvents = true
        let html = #"""
        <!doctype html><meta charset=utf-8><meta http-equiv=Content-Security-Policy content="default-src 'none'; script-src 'unsafe-inline'; frame-src about:"><body><script>
        for (const kind of ['normal','same-document-beforeinput','cross-frame-beforeinput','cross-frame-textInput',
          'cross-frame-before-command','reentrant-cross-frame','adopt-leaf-cross-frame','cancel','replace-leaf']) {
          const section=document.createElement('section');section.id=kind;document.body.append(section);
          section.innerHTML='<div data-unit contenteditable=true><span data-leaf contenteditable=true>unit original</span><b data-sibling contenteditable=false>Formatted sibling</b></div><input data-decoy value="decoy original"><iframe></iframe>';
          const unit=section.querySelector('[data-unit]'), leaf=section.querySelector('[data-leaf]');
          const sibling=section.querySelector('[data-sibling]'), decoy=section.querySelector('[data-decoy]');
          const child=section.querySelector('iframe'), childDocument=child.contentDocument;
          childDocument.body.innerHTML='<input id=child-editor value="child original">';
          childDocument.body.setAttribute('data-case',kind);
          const childEditor=childDocument.getElementById('child-editor');
          let counts={before:0,input:0,textInput:0},reentered=false;
          unit.setAttribute('data-model','unit original');unit.setAttribute('data-rerenders','0');
          section.setAttribute('data-counts',JSON.stringify(counts));
          function retargetChild(){childEditor.focus();childEditor.select();}
          for (const type of ['beforeinput','input','textInput']) unit.addEventListener(type,e=>{
            counts[type==='beforeinput'?'before':type]++;section.setAttribute('data-counts',JSON.stringify(counts));
            if(type==='beforeinput'){
              if(kind==='same-document-beforeinput'){decoy.focus();decoy.select();}
              if(kind==='cross-frame-beforeinput')retargetChild();
              if(kind==='reentrant-cross-frame'&&!reentered){reentered=true;retargetChild();childDocument.execCommand('insertText',false,'page reentrant');}
              if(kind==='adopt-leaf-cross-frame'){childDocument.body.append(childDocument.adoptNode(leaf));leaf.focus();const r=childDocument.createRange();r.selectNodeContents(leaf);const s=child.contentWindow.getSelection();s.removeAllRanges();s.addRange(r);}
              if(kind==='cancel')e.preventDefault();
              if(kind==='replace-leaf'){const next=leaf.cloneNode(true);leaf.replaceWith(next);next.focus();const r=document.createRange();r.selectNodeContents(next);getSelection().removeAllRanges();getSelection().addRange(r);}
            }
            if(type==='textInput'&&kind==='cross-frame-textInput')retargetChild();
            if(type==='input'&&e.isTrusted){
              // Framework-like delegated ownership: read the logical editor,
              // excluding its exact formatted noneditable sibling, then replace
              // the leaf from model state. No retained old-leaf read is used.
              const model=Array.from(unit.childNodes).filter(n=>n!==sibling).map(n=>n.textContent).join('');
              unit.setAttribute('data-model',model);
              queueMicrotask(()=>{
                const next=document.createElement('span');next.contentEditable='true';next.setAttribute('data-leaf','');next.textContent=model;
                unit.replaceChildren(next,sibling);unit.setAttribute('data-rerenders',String(Number(unit.getAttribute('data-rerenders'))+1));
              });
              window.open('about:blank','_blank');
            }
          });
        }
        </script>
        """#
        view.loadHTMLString(html,baseURL:URL(string:"https://fixed.invalid/"))
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard message.webView === view, let body=message.body as? [String:Any],
              let kind=body["kind"] as? String,
              let bytes=try? JSONSerialization.data(withJSONObject:body),bytes.count<30000 else { fail("message") }
        if kind == "error" { fail("fixture script error") }
        if kind == "event" {
            guard events.count<180 else { fail("event bound") }
            let origin=message.frameInfo.securityOrigin
            events.append(["witness":body,"mainFrame":message.frameInfo.isMainFrame,
                "nativeOrigin":"\(origin.protocol)://\(origin.host):\(origin.port)",
                "nativeFrameURL":message.frameInfo.request.url?.absoluteString ?? "absent"])
            return
        }
        guard kind=="complete",message.frameInfo.isMainFrame,
              let results=body["results"] as? [[String:Any]],results.count==9,
              view.url?.absoluteString=="https://fixed.invalid/",!window.isVisible,!window.isKeyWindow,
              !window.isMainWindow,!NSApplication.shared.isActive else { fail("completion") }
        validate(results)
        let output:[String:Any]=["results":results,"events":events,"navigations":navigations,"popupCount":popupCount,
            "nativeURL":view.url!.absoluteString,"appActive":NSApplication.shared.isActive,"windowVisible":window.isVisible,
            "windowKey":window.isKeyWindow,"windowMain":window.isMainWindow]
        let data=try! JSONSerialization.data(withJSONObject:output,options:[.prettyPrinted,.sortedKeys])
        print(String(data:data,encoding:.utf8)!)
        view.configuration.userContentController.removeScriptMessageHandler(forName:"witness",contentWorld:WKContentWorld.world(name:"trusted-document-command-proof"))
        view.stopLoading();window.contentView=nil;window.close();view=nil;window=nil
        NSApplication.shared.terminate(nil)
    }
    func validate(_ results:[[String:Any]]) {
        let expected=["normal","same-document-beforeinput","cross-frame-beforeinput","cross-frame-textInput",
            "cross-frame-before-command","reentrant-cross-frame","adopt-leaf-cross-frame","cancel","replace-leaf"]
        guard results.compactMap({$0["kind"] as? String})==expected,popupCount==0 else { fail("case or popup inventory") }
        for (index,row) in results.enumerated() {
            guard row["valid"] as? Bool==true,row["returned"] as? Bool==true,row["error"] is NSNull,
                  row["siblingIntact"] as? Bool==true,row["mainDocumentStillExact"] as? Bool==true,
                  row["active"] as? Bool==false,row["sticky"] as? Bool==false,
                  row["decoy"] as? String=="decoy original" else { fail("row invariants") }
            let counts=row["handlerCounts"] as? [String:Int]
            guard counts?["before"]==1,counts?["textInput"]==0 else { fail("event inventory") }
            if index<6 {
                guard row["model"] as? String=="document replacement",row["freshValue"] as? String=="document replacement",
                      row["rerender"] as? Int==1,row["leafReplaced"] as? Bool==true,counts?["input"]==1,
                      row["child"] as? String==(index==5 ? "page reentrant" : "child original")
                else { fail("logical editor model retention") }
            } else if index==6 {
                guard row["model"] as? String=="",row["freshValue"] as? String=="",counts?["input"]==1,
                      row["originalLeafValue"] as? String=="document replacement",
                      row["originalLeafDocumentMain"] as? Bool==false else { fail("adopted-node counterexample") }
            } else {
                guard row["model"] as? String=="unit original",row["freshValue"] as? String=="unit original",
                      row["rerender"] as? Int==0,counts?["input"]==0 else { fail("cancelled or replaced target") }
            }
        }
        let childInputs=events.filter {
            $0["mainFrame"] as? Bool==false && ($0["witness"] as? [String:Any])?["type"] as? String=="input"
        }
        guard childInputs.count==2 else { fail("child effect inventory") }
        for (row,data) in [("reentrant-cross-frame","page reentrant"),("adopt-leaf-cross-frame","document replacement")] {
            guard childInputs.contains(where:{
                let witness=$0["witness"] as? [String:Any]
                return witness?["row"] as? String==row && witness?["data"] as? String==data
                    && witness?["trusted"] as? Bool==true && $0["nativeOrigin"] as? String=="https://fixed.invalid:0"
            }) else { fail("native child frame event join") }
        }
        guard events.allSatisfy({
            let witness=$0["witness"] as? [String:Any]
            return witness?["trusted"] as? Bool==true && witness?["active"] as? Bool==false && witness?["sticky"] as? Bool==false
        }) else { fail("event trust or activation") }
    }
    func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction, decisionHandler: @escaping (WKNavigationActionPolicy)->Void) {
        let url=action.request.url?.absoluteString ?? "absent"
        let main=action.targetFrame?.isMainFrame ?? false
        let allow=navigations.isEmpty&&main&&url=="https://fixed.invalid/"
        guard navigations.count<32 else { decisionHandler(.cancel);fail("navigation bound") }
        navigations.append(["url":url,"mainFrame":main,"allowed":allow]);decisionHandler(allow ? .allow : .cancel)
    }
    func webView(_ webView: WKWebView,createWebViewWith configuration: WKWebViewConfiguration,for action: WKNavigationAction,windowFeatures: WKWindowFeatures)->WKWebView? { popupCount+=1;return nil }
}
func fail(_ reason:String)->Never{fputs("document command proof failed: \(reason)\n",stderr);exit(1)}
let app=NSApplication.shared
app.setActivationPolicy(.accessory);app.finishLaunching()
let probe=DocumentCommandProbe();probe.start()
DispatchQueue.main.asyncAfter(deadline:.now()+15){fail("deadline")}
app.run()
