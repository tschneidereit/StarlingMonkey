import {
  DebuggerMessage,
  DebuggerMessageBreakpointSet,
  DebuggerMessageBreakpointsForLine,
  DebuggerMessageConnect,
  DebuggerMessageProgramLoaded,
  DebuggerMessageScopes,
  DebuggerMessageStack,
  DebuggerMessageStopOnBreakpoint,
  DebuggerMessageStopOnStep,
  DebuggerMessageType,
  DebuggerMessageVariables,
  DebuggerMessageVariableSet,
  HostMessage,
  HostMessageType,
  SourceLocation,
} from "../shared/protocol-types";

import { DebugProtocol } from "@vscode/debugprotocol";
import { Debugger } from "./spidermonkey-debugger";
import { StackFrame } from "@vscode/debugadapter/lib/debugSession";

declare const socket: {
  send(data: string): void;
  receive(bytes: number): string;
};

declare function print(message: string): void;
declare function assert(condition: any, message?: string): asserts condition;
declare function setContentPath(path: string): void;

let LOG = false;

try {
  let dbg = new Debugger();
  dbg.addAllGlobalsAsDebuggees();

  let scripts = new Map<string, Set<Debugger.Script>>();
  let currentFrame: Debugger.Frame | undefined;
  let lastLine = 0;
  let lastColumn = 0;
  // Reserve the first 0xFFF for stack frames.
  const MAX_FRAMES = 0xfff;
  const GLOBAL_OBJECT_REF = MAX_FRAMES + 1;
  const OBJECT_REFS_START = GLOBAL_OBJECT_REF + 1;
  let varRefsIndex = OBJECT_REFS_START;
  let objectToId = new Map<Debugger.Object, number>();
  let idToObject = new Map<number, Debugger.Object>();
  let breakpointId = 0;

  function addScript(script: Debugger.Script): void {
    let urlScripts = scripts.get(script.url);
    if (!urlScripts) {
      urlScripts = new Set();
      scripts.set(script.url, urlScripts);
    }
    urlScripts.add(script);
  }

  dbg.onNewScript = function (script: Debugger.Script, _global?: any): void {
    if (scripts.has(script.url)) {
      LOG && print(`Warning: script with url ${script.url} already loaded`);
    }
    addScript(script);
  };

  dbg.onEnterFrame = function (frame: Debugger.Frame): void {
    dbg.onEnterFrame = undefined;
    let path = frame.script.url;
    for (let script of dbg.findScripts()) {
      addScript(script);
    }
    LOG && print(`Loaded script ${frame.script.url}`);
    sendMessage({ type: DebuggerMessageType.ProgramLoaded, source: { path } });
    return handlePausedFrame(frame);
  };

  function handlePausedFrame(frame: Debugger.Frame): void {
    try {
      dbg.onEnterFrame = undefined;
      if (currentFrame) {
        currentFrame.onStep = undefined;
        currentFrame.onPop = undefined;
      }
      currentFrame = frame;
      idToObject.set(GLOBAL_OBJECT_REF, frame.script.global);
      objectToId.set(frame.script.global, GLOBAL_OBJECT_REF);
      varRefsIndex = OBJECT_REFS_START;
      setCurrentPosition();
      waitForSocket();
      objectToId.clear();
      idToObject.clear();
    } catch (e) {
      assert(e instanceof Error);
      LOG &&
        print(
          `Exception during paused frame handling: ${e}. Stack:\n${e.stack}`
        );
    }
  }

  function waitForSocket(): void {
    while (true) {
      try {
        let message = receiveMessage();
        LOG && print(`received message ${JSON.stringify(message)}`);
        switch (message.type) {
          case HostMessageType.LoadProgram:
            setContentPath(message.value);
            return;
          case HostMessageType.GetBreakpointsForLine:
            getBreakpointsForLine(message.value);
            break;
          case HostMessageType.SetBreakpoint:
            setBreakpoint(message.value);
            break;
          case HostMessageType.GetStack:
            getStack(message.value.index, message.value.count);
            break;
          case HostMessageType.GetScopes:
            getScopes(message.value);
            break;
          case HostMessageType.GetVariables:
            getVariables(message.value);
            break;
          case HostMessageType.SetVariable:
            setVariable(message.value);
            break;
          case HostMessageType.Next:
            currentFrame!.onStep = handleNext;
            return;
          case HostMessageType.StepIn:
            currentFrame!.onStep = handleNext;
            dbg.onEnterFrame = handleStepIn;
            return;
          case HostMessageType.StepOut:
            currentFrame!.onPop = handleStepOut;
            return;
          case HostMessageType.Continue:
            currentFrame = undefined;
            return;
          case HostMessageType.StartDebugLogging:
            LOG = true;
            break;
          case HostMessageType.StopDebugLogging:
            LOG = false;
            break;
          default:
            LOG &&
              print(
                `Invalid message received, continuing execution. Message: ${message.type}`
              );
            currentFrame = undefined;
            return;
        }
      } catch (e) {
        assert(e instanceof Error);
        LOG &&
          print(`Exception during paused frame loop: ${e}. Stack:\n${e.stack}`);
      }
    }
  }

  function setCurrentPosition(): void {
    if (!currentFrame) {
      lastLine = 0;
      lastColumn = 0;
      return;
    }
    let offsetMeta = currentFrame.script.getOffsetMetadata(currentFrame.offset);
    lastLine = offsetMeta.lineNumber;
    lastColumn = offsetMeta.columnNumber;
  }

  function positionChanged(frame: Debugger.Frame): boolean {
    let offsetMeta = frame.script.getOffsetMetadata(frame.offset);
    return (
      offsetMeta.lineNumber !== lastLine ||
      offsetMeta.columnNumber !== lastColumn
    );
  }

  function handleNext(this: Debugger.Frame): void {
    if (!positionChanged(this)) {
      return;
    }
    sendMessage({ type: DebuggerMessageType.StopOnStep,  });
    handlePausedFrame(this);
  }

  function handleStepIn(frame: Debugger.Frame): void {
    dbg.onEnterFrame = undefined;
    sendMessage({ type: DebuggerMessageType.StopOnStep,  });
    handlePausedFrame(frame);
  }

  function handleStepOut(this: Debugger.Frame): void {
    this.onPop = undefined;
    if (this.older) {
      this.older.onStep = handleNext;
    } else {
      dbg.onEnterFrame = handleStepIn;
    }
  }

  const breakpointHandler: Debugger.BreakpointHandler = {
    hit(frame: Debugger.Frame): void {
      // TODO: get the breakpoint ID instead of using offset here
      sendMessage({ type: DebuggerMessageType.StopOnBreakpoint, id: frame.offset });
      return handlePausedFrame(frame);
    },
  };

  interface InternalSourceLocation extends SourceLocation {
    script: Debugger.Script;
    offset: number;
  }

  function getPossibleBreakpointsInScripts(
    scripts: Set<Debugger.Script> | undefined,
    line: number,
    column: number,
  ): InternalSourceLocation[] {
    let locations = [];
    for (let script of scripts ?? []) {
      getPossibleBreakpointsInScriptRecursive(script, line, column, locations);
    }
    return locations;
  }

  function getPossibleBreakpointsInScriptRecursive(
    script: Debugger.Script,
    line: number,
    column: number,
    locations: InternalSourceLocation[]
  ) {
    let offsets = script.getPossibleBreakpointOffsets({
      line,
      minColumn: column,
    });
    for (let offset of offsets) {
      let meta = script.getOffsetMetadata(offset);
      locations.push({
        script,
        offset,
        line: meta.lineNumber,
        column: meta.columnNumber,
      });
    }

    for (let child of script.getChildScripts()) {
      getPossibleBreakpointsInScriptRecursive(child, line, column, locations);
    }
  }

  function getBreakpointsForLine({
    path,
    line,
    column,
  }: {
    path: string;
    line: number;
    column: number;
  }): void {
    let fileScripts = scripts.get(path);
    let locations = getPossibleBreakpointsInScripts(fileScripts, line, column);
    let externalLocations = locations.map(({ line, column}) => ({
      line,
      column,
    }));
    sendMessage({ type: DebuggerMessageType.BreakpointsForLine, locations: externalLocations });
  }

  function setBreakpoint({
    path,
    line,
    column,
  }: {
    path: string;
    line: number;
    column: number;
  }): void {
    let fileScripts = scripts.get(path);
    if (!fileScripts) {
      LOG && print(`Can't set breakpoint: no scripts found for file ${path}`);
      sendMessage({ type: DebuggerMessageType.BreakpointSet, id: -1, location: { line, column } });
      return;
    }
    let locations = getPossibleBreakpointsInScripts(fileScripts, line, column);
    let location;
    for (location of locations) {
      if (location.line === line && location.column === column) {
        location.script.setBreakpoint(location.offset, breakpointHandler);
        break;
      }
    }
    sendMessage({ type: DebuggerMessageType.BreakpointSet, id: ++breakpointId, location: { line, column } });
  }

  function getStack(index: number, count: number): void {
    let stack: DebugProtocol.StackFrame[] = [];
    assert(currentFrame);
    let frame = findFrame(currentFrame, index);

    while (stack.length < count) {
      let entry = new StackFrame(stack.length, frame.callee ? frame.callee.name : frame.type);
      if (frame.script) {
        const offsetMeta = frame.script.getOffsetMetadata(frame.offset);
        entry.source = { path: frame.script.url };
        entry.line = offsetMeta.lineNumber;
        entry.column = offsetMeta.columnNumber;
      }

      stack.push(entry);
      let nextFrame = frame.older || frame.olderSavedFrame;
      if (!nextFrame) {
        break;
      }
      frame = nextFrame;
    }
    sendMessage({ type: DebuggerMessageType.Stack, stack });
  }

  function getScopes(index: number): void {
    assert(currentFrame);
    let frame = findFrame(currentFrame, index);
    let script = frame.script;
    let scopes: DebugProtocol.Scope[] = [
      {
        name: "Locals",
        presentationHint: "locals",
        variablesReference: index + 1,
        expensive: false,
        line: script.startLine,
        column: script.startColumn,
        endLine: script.startLine + script.lineCount,
      },
      {
        name: "Globals",
        presentationHint: "globals",
        variablesReference: GLOBAL_OBJECT_REF,
        expensive: true,
      },
    ];

    sendMessage({ type: DebuggerMessageType.Scopes, scopes });
  }

  function getVariables(reference: number): void {
    if (reference > MAX_FRAMES) {
      let object = idToObject.get(reference);
      let variables = getMembers(object!);
      sendMessage({ type: DebuggerMessageType.Variables, variables });
      return;
    }

    assert(currentFrame);
    let frame = findFrame(currentFrame, reference - 1);
    let variables: DebugProtocol.Variable[] = [];

    for (let name of frame.environment.names()) {
      let value = frame.environment.getVariable(name);
      variables.push({ name, ...formatValue(value) });
    }

    if (frame.this) {
      let { value, type, variablesReference } = formatValue(frame.this);
      variables.push({
        name: "<this>",
        value,
        type,
        variablesReference,
      });
    }

    sendMessage({ type: DebuggerMessageType.Variables, variables });
  }

  function setVariable({
    variablesReference,
    name,
    value,
  }: {
    variablesReference: number;
    name: string;
    value: any;
  }): void {
    let newValue;
    if (variablesReference > MAX_FRAMES) {
      let object = idToObject.get(variablesReference);
      assert(object);
      object.setProperty(name, value);
      newValue = getMember(object, name);
    } else {
      assert(currentFrame);
      let frame = findFrame(currentFrame, variablesReference - 1);
      frame.environment.setVariable(name, value);
      newValue = formatValue(frame.environment.getVariable(name));
    }
    sendMessage({ type: DebuggerMessageType.VariableSet, newValue });
  }

  function getMembers(object: Debugger.Object): DebugProtocol.Variable[] {
    let names = object.getOwnPropertyNames();
    let members: DebugProtocol.Variable[] = [];
    for (let name of names) {
      members.push(getMember(object, name));
    }
    return members;
  }

  function getMember(object: Debugger.Object, name: string): DebugProtocol.Variable {
    let descriptor = object.getOwnPropertyDescriptor(name);
    return { name, ...formatDescriptor(descriptor) };
  }

  function formatValue(value: any): {
    value: string;
    type: string;
    variablesReference: number;
  } {
    let formatted;
    let type: string = typeof value;
    let structured = false;
    type = type[0].toUpperCase() + type.slice(1);
    if (type === "Object") {
      if (value === null) {
        formatted = "null";
        type = "Null";
      } else if (!(value instanceof Debugger.Object) && value.uninitialized) {
        formatted = "<uninitialized>";
        type = "Uninitialized Binding";
      } else {
        type = value.class ?? "Object";
        formatted = `[object ${type}]`;
        structured = true;
      }
    } else if (type === "String") {
      formatted = `"${value}"`;
    } else {
      formatted = `${value}`;
    }
    let variablesReference = 0;
    if (structured) {
      if (!objectToId.has(value)) {
        variablesReference = varRefsIndex++;
        idToObject.set(variablesReference, value);
        objectToId.set(value, variablesReference);
      }
    }
    return { value: formatted, type, variablesReference };
  }

  function formatDescriptor(descriptor: Debugger.PropertyDescriptor): {
    value: string;
    type: string;
    variablesReference: number;
  } {
    if (descriptor.value) {
      return formatValue(descriptor.value);
    }

    let formatted;
    if (descriptor.get) {
      formatted = formatValue(descriptor.get);
    }

    if (descriptor.set) {
      let setter = formatValue(descriptor.set);
      if (formatted) {
        formatted += `, ${setter}`;
      } else {
        formatted = setter;
      }
    }

    return { value: formatted, type: "Accessor", variablesReference: 0 };
  }

  function findFrame(start: Debugger.Frame, index: number): Debugger.Frame {
    let frame = start;
    for (let i = 0; i < index && frame; i++) {
      let nextFrame = frame.older || frame.olderSavedFrame;
      frame = nextFrame!;
      assert(frame, `Frame with index ${index} not found`);
    }
    return frame;
  }

  function sendMessage(message: DebuggerMessage) {
    const messageStr = JSON.stringify(message);
    LOG && print(`sending message: ${messageStr}`);
    socket.send(`${messageStr.length}\n${messageStr}`);
  }

  function receiveMessage(): HostMessage {
    LOG && print("Debugger listening for incoming message ...");
    let partialMessage = "";
    let eol = -1;
    while (true) {
      partialMessage += socket.receive(10);
      eol = partialMessage.indexOf("\n");
      if (eol >= 0) {
        break;
      }
    }

    let length = parseInt(partialMessage.slice(0, eol), 10);
    if (isNaN(length)) {
      LOG &&
        print(
          `WARN: Received message ${partialMessage} not of the format '[length]\\n[JSON encoded message with length {length}]', discarding`
        );
      return receiveMessage();
    }
    partialMessage = partialMessage.slice(eol + 1);

    while (partialMessage.length < length) {
      partialMessage += socket.receive(length - partialMessage.length);
    }

    if (partialMessage.length > length) {
      LOG &&
        print(
          `WARN: Received message ${
            partialMessage.length - length
          } bytes longer than advertised, ignoring everything beyond the first ${length} bytes`
        );
      partialMessage = partialMessage.slice(0, length);
    }

    try {
      return JSON.parse(partialMessage);
    } catch (e) {
      assert(e instanceof Error);
      LOG &&
        print(
          `WARN: Ill-formed message received, discarding: ${e}, ${e.stack}`
        );
      return receiveMessage();
    }
  }

  sendMessage({ type: DebuggerMessageType.Connect,  });
  waitForSocket();
} catch (e) {
  assert(e instanceof Error);
  LOG &&
    print(
      `Setting up connection to debugger failed with exception: ${e},\nstack:\n${e.stack}`
    );
}
