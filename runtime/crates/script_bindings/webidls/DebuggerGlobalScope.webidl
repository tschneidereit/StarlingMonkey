/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// This interface is entirely internal to StarlingMonkey, and should not be accessible to
// content.
// https://html.spec.whatwg.org/multipage/#dedicatedworkerglobalscope
[Global=DebuggerGlobalScope, Exposed=DebuggerGlobalScope]
/*sealed*/ interface DebuggerGlobalScope : GlobalScope {
};
