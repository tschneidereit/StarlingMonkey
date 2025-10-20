/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Unused dummy scope. This exists so that bindings for interfaces targeting
// multiple global scopes contain references to `GlobalScope` instead of
// `StarlingGlobalScope`.
// https://html.spec.whatwg.org/multipage/#dedicatedworkerglobalscope
[Global=(Window, DissimilarOriginWindow), Exposed=DebuggerGlobalScope]
/*sealed*/ interface Window : GlobalScope {
};
