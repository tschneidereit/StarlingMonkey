/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// A global scope for StarlingMonkey, and a reduced version of DedicatedWorkerGlobalScope.
// This pretends to be all the global scopes, because that way other WebIDLs don't have to be adjusted
// to not mention any of these scopes.
// https://html.spec.whatwg.org/multipage/#dedicatedworkerglobalscope
[Global=(Worker, DedicatedWorker, Worklet, PaintWorklet, DebuggerGlobalScope), Exposed=DedicatedWorker]
/*sealed*/ interface StarlingGlobalScope : WorkerGlobalScope {
};
