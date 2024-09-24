try {
  let dbg = new Debugger();
  dbg.addAllGlobalsAsDebuggees();

  let mainScript;
  let currentFrame;
  let lastLine = 0;
  let lastColumn = 0;
  // Reserve the first 0xFFF for stack frames.
  const MAX_FRAMES = 0xFFF - 1;
  const GLOBAL_OBJECT_REF = 0xFFF;
  const OBJECT_REFS_START = GLOBAL_OBJECT_REF + 1;
  let varRefsIndex = OBJECT_REFS_START;
  let varRefs = new Map();

  dbg.onNewScript = function (script, global) {
    if (mainScript) {
      print(
        "Main script already loaded, ignoring additional scripts for now. (This should change.)"
      );
      return;
    }

    mainScript = script;
    scriptGlobal = global;
  };

  dbg.onEnterFrame = function (frame) {
    if (frame.type !== "module") {
      return;
    }
    let { url, lineCount } = frame.script;
    sendMessage("programLoaded", { url, lineCount });
    return handlePausedFrame(frame);
  };

  function handlePausedFrame(frame) {
    try {
      dbg.onEnterFrame = undefined;
      if (currentFrame) {
        currentFrame.onStep = undefined;
        currentFrame.onPop = undefined;
      }
      currentFrame = frame;
      varRefs.set(GLOBAL_OBJECT_REF, frame.script.global);
      varRefsIndex = OBJECT_REFS_START;
      setCurrentPosition();
      waitForSocket();
      varRefs.clear();
    } catch (e) {
      print(`Exception during paused frame handling: ${e}. Stack:\n${e.stack}`);
    }
  }

  function waitForSocket() {
    while (true) {
      try {
        let message = receiveMessage();
        switch (message.type) {
          case "loadProgram":
            setContentPath(message.value);
            return;
          case "getBreakpointsForLine":
            getBreakpointsForLine(message.value);
            break;
          case "setBreakpoint":
            setBreakpoint(message.value);
            break;
          case "getStack":
            getStack(message.value.index, message.value.count);
            break;
          case "getScopes":
            getScopes(message.value);
            break;
          case "getVariables":
            getVariables(message.value);
            break;
          case "setVariable":
            setVariable(message.value);
            break;
          case "next":
            currentFrame.onStep = handleNext;
            return;
          case "stepIn":
            currentFrame.onStep = handleNext;
            dbg.onEnterFrame = handleStepIn;
            return;
          case "stepOut":
            currentFrame.onPop = handleStepOut;
            return;
          case "continue":
            currentFrame = undefined;
            return;
          default:
            print(
              `Invalid message received, continuing execution. Message: ${message.type}`
            );
            currentFrame = undefined;
            return;
        }
      } catch (e) {
        print(`Exception during paused frame loop: ${e}. Stack:\n${e.stack}`);
      }
    }
  }

  function setCurrentPosition() {
    if (!currentFrame) {
      lastLine = 0;
      lastColumn = 0;
      return;
    }
    let offsetMeta = currentFrame.script.getOffsetMetadata(
      currentFrame.offset
    );
    lastLine = offsetMeta.lineNumber;
    lastColumn = offsetMeta.columnNumber;
  }

  function positionChanged(frame) {
    let offsetMeta = frame.script.getOffsetMetadata(frame.offset);
    return offsetMeta.lineNumber !== lastLine || offsetMeta.columnNumber !== lastColumn;
  }

  function handleNext() {
    if (!positionChanged(this)) {
      return;
    }
    sendMessage("stopOnStep");
    handlePausedFrame(this);
  }

  function handleStepIn(frame) {
    dbg.onEnterFrame = undefined;
    sendMessage("stopOnStep");
    handlePausedFrame(frame);
  }

  function handleStepOut(reason) {
    this.onPop = undefined;
    if (this.older) {
      this.older.onStep = handleNext;
    } else {
      dbg.onEnterFrame = handleStepIn;
    }
  }

  function finishHandleStepOut() {
    this.onStep = undefined;
    sendMessage("stopOnStep");
    handlePausedFrame(this);
  }

  const breakpointHandler = {
    hit(frame) {
      sendMessage("breakpointHit", frame.offset);
      return handlePausedFrame(frame);
    },
  };

  function getPossibleBreakpointsInScriptOrChild(script, line) {
    let offsets = script.getPossibleBreakpointOffsets({ line });
    if (offsets.length) {
      return { script, offsets };
    }

    for (let child of script.getChildScripts()) {
      let result = getPossibleBreakpointsInScriptOrChild(child, line);
      if (result) {
        return result;
      }
    }
    return null;
  }

  function getBreakpointsForLine({file, line}) {
    // TODO: support multiple files.
    let { script, offsets } = getPossibleBreakpointsInScriptOrChild(mainScript, line) || {};
    if (offsets) {
      offsets = offsets.map(offset => {
        let meta = script.getOffsetMetadata(offset);
        return {
          line: meta.lineNumber,
          column: meta.columnNumber,
        };
      });
    }
    sendMessage("breakpointsForLine", offsets);
  }

  function setBreakpoint({file, line, column}) {
    // TODO: support multiple files.
    let { script, offsets } = getPossibleBreakpointsInScriptOrChild(
      mainScript,
      line
    ) || {};
    let offset = -1;
    if (offsets) {
      for (offset of offsets) {
        let meta = script.getOffsetMetadata(offset);
        assert(meta.lineNumber === line, `Line number mismatch, should be ${line}, got ${meta.lineNumber}`);
        if (meta.columnNumber === column) {
          break;
        }
      }
      script.setBreakpoint(offset, breakpointHandler);
    }
    sendMessage("breakpointSet", { id: offset, line, column });
  }

  /**
   * Sends a stack to the debugger as an array of objects with the following interface:
   * interface IRuntimeStackFrame {
   *   index: number;
   *   name: string;
   *   file: string;
   *   line: number;
   *   column?: number;
   *   instruction?: number;
   *}
   */
  function getStack(index, count) {
    let stack = [];
    let frame = findFrame(currentFrame, index);

    while (frame && stack.length < count) {
      const offsetMeta = frame.script.getOffsetMetadata(frame.offset);
      let entry = {
        index: stack.length,
        file: frame.script.url,
        line: offsetMeta.lineNumber,
        column: offsetMeta.columnNumber,
      };
      if (frame.callee) {
        entry.name = frame.callee.name;
      } else {
        entry.name = frame.type;
      }
      stack.push(entry);
      let nextFrame = frame.older || frame.olderSavedFrame;
      frame = nextFrame;
    }
    sendMessage("stack", stack);
  }

  /**
   * Sends a list of scopes to the debugger as an array of objects with the following interface:
   * interface Scope {
   *     name: string;
   *     presentationHint?: 'arguments' | 'locals' | 'registers' | string;
   *     variablesReference: number;
   *     namedVariables?: number;
   *     indexedVariables?: number;
   *     expensive: boolean;
   *     source?: Source;
   *     line?: number;
   *     column?: number;
   *     endLine?: number;
   *     endColumn?: number;
   * }
   */
  function getScopes(index) {
    let scopes = [];
    let frame = findFrame(currentFrame, index);
    let script = frame.script;
    scopes.push({
      name: "Locals",
      presentationHint: "locals",
      variablesReference: index + 1,
      expensive: false,
      line: script.startLine,
      column: script.startColumn,
      endLine: script.startLine + script.lineCount,
    });
    scopes.push({
      name: "Globals",
      presentationHint: "globals",
      variablesReference: GLOBAL_OBJECT_REF,
      expensive: true,
    });

    sendMessage("scopes", scopes);
  }

  function getVariables(reference) {
    if (reference > MAX_FRAMES) {
      let object = varRefs.get(reference);
      let locals = getMembers(object);
      sendMessage("variables", locals);
      return;
    }

    let frame = findFrame(currentFrame, reference - 1);

    locals = [];
    for (let name of frame.environment.names()) {
      let value = frame.environment.getVariable(name);
      locals.push({ name, ...formatValue(value) });
    }
    if (frame.this) {
      let { value, type, variablesReference } = formatValue(frame.this);
      locals.push({
        name: "<this>",
        value,
        type,
        variablesReference,
      });
    }

    sendMessage("variables", locals);
  }

  function setVariable({ variablesReference, name, value }) {
    let newValue;
    if (variablesReference > MAX_FRAMES) {
      let object = varRefs.get(variablesReference);
      object.setProperty(name, value);
      newValue = getMember(object, name);
    } else {
      let frame = findFrame(currentFrame, variablesReference - 1);
      frame.environment.setVariable(name, value);
      newValue = formatValue(frame.environment.getVariable(name));
    }
    sendMessage("variableSet", newValue);
  }

  function getMembers(object) {
    let names = object.getOwnPropertyNames();
    let members = [];
    for (let name of names) {
      members.push(getMember(object, name));
    }
    return members;
  }

  function getMember(object, name) {
    let descriptor = object.getOwnPropertyDescriptor(name);
    return { name, ...formatDescriptor(descriptor) };
  }

  function formatValue(value) {
    let formatted;
    let type = typeof value;
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
      variablesReference = varRefs.get(value) ?? varRefsIndex++;
      varRefs.set(variablesReference, value);
      varRefs.set(value, variablesReference);
    }
    return { value: formatted, type, variablesReference };
  }

  function formatDescriptor(descriptor) {
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

  function findFrame(start, index) {
    let frame = start;
    for (let i = 0; i < index && frame; i++) {
      let nextFrame = frame.older || frame.olderSavedFrame;
      frame = nextFrame;
    }
    return frame;
  }

  function sendMessage(type, value) {
    const messageStr = JSON.stringify({ type, value });
    // print(`sending message: ${messageStr}`);
    socket.send(`${messageStr.length}\n`);
    socket.send(messageStr);
  }

  function receiveMessage() {
    print("Debugger listening for incoming message ...");
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
      print(`WARN: Received message ${partialMessage} not of the format '[length]\\n[JSON encoded message with length {length}]', discarding`);
      return receiveMessage();
    }
    partialMessage = partialMessage.slice(eol + 1);

    while (partialMessage.length < length) {
      partialMessage += socket.receive(length - partialMessage.length);
    }

    if (partialMessage.length > length) {
      print(`WARN: Received message is too long, ignoring everything beyond the first ${length} bytes`);
      partialMessage = partialMessage.slice(0, length);
    }

    try {
      let message = JSON.parse(partialMessage);
      // print(`Received message: ${partialMessage}`);
      return message;
    } catch (e) {
      print(`WARN: Illformed message received, discarding: ${e}`);
      return receiveMessage();
    }
  }

  sendMessage("connect");
  waitForSocket();

} catch (e) {
  print("Setting up connection to debugger failed with exception:");
  print(e, e.stack);
}
