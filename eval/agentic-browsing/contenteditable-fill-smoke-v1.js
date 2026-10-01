"use strict";

// Fill now lives inside the admitted isolated runtime, not a page-world relay.
// Exercise its actual ref/descriptor/action path, including native primitive
// capture, contenteditable values, credential/rewrite refusal and post-mutation
// exception classification. The real WKWebView authority traps are separate.
require("./semantic-runtime-smoke-v1.js");
