import { Scope } from "@vscode/debugadapter";
import { EventEmitter } from "events";
import * as Net from "net";
import { Signal } from "./signals.js";
import assert from "node:assert/strict";
import { Terminal, TerminalShellExecution, window } from "vscode";
import { ILaunchRequestArguments } from "./starlingMonkeyDebugger.js";
import { SourceLocation, SourceMaps } from "./sourcemaps/sourceMaps.js";
import { dirname } from "path";
import { DebuggerMessage, DebuggerMessageStack, DebuggerMessageType, HostMessageType } from "../shared/protocol-types";
import { DebugProtocol } from "@vscode/debugprotocol";

export interface FileAccessor {
  isWindows: boolean;
  readFile(path: string): Promise<Uint8Array>;
  writeFile(path: string, contents: Uint8Array): Promise<void>;
}

export interface IRuntimeBreakpoint {
  id: number;
  line: number;
  column: number;
}

interface IRuntimeVariable {
  name: string;
  value: string;
  type: string;
  variablesReference: number;
}

export interface IComponentRuntimeConfig {
  executable: string;
  options: string[];
  envOption: string;
}
export interface IStarlingMonkeyRuntimeConfig {
  jsRuntimeOptions: string[];
  componentRuntime: IComponentRuntimeConfig;
  trace: boolean;
}

class ComponentRuntimeInstance {
  private static _workspaceFolder?: string;
  private static _server?: Net.Server;
  private static _terminal?: Terminal;
  private static _nextSessionPort?: number;
  private static _runtimeExecution?: TerminalShellExecution;
  private static _execOptions?: string;

  static setNextSessionPort(port: number) {
    this._nextSessionPort = port;
  }

  static async start(
    workspaceFolder: string,
    config: IStarlingMonkeyRuntimeConfig,
    component: string,
  ) {
    this.applyConfig(workspaceFolder, config, component);
    this.ensureServer();
    this.ensureHostRuntime(config, workspaceFolder, component);
  }
  static applyConfig(workspaceFolder: string, config: IStarlingMonkeyRuntimeConfig, component: string) {
    if (this._workspaceFolder && this._workspaceFolder !== workspaceFolder) {
      this._server?.close();
      this._server = undefined;
      this._nextSessionPort = undefined;
    }

    let execOptions = config.componentRuntime.executable;
    execOptions += config.componentRuntime.options.join(" ");
    execOptions += config.componentRuntime.envOption;
    execOptions += config.jsRuntimeOptions.join("");
    execOptions += component;

    if (this._runtimeExecution && this._execOptions! !== execOptions) {
      this._terminal?.dispose();
      this._terminal = undefined;
      this._runtimeExecution = undefined;
    }

    this._workspaceFolder = workspaceFolder;
    this._execOptions = execOptions;
  }

  static ensureServer() {
    if (this._server) {
      return;
    }
    this._server = Net.createServer((socket) => {
      socket.on("data", (data) => {
        assert.equal(
          data.toString(), "get-session-port",
          `expected "get-session-port" message, got "${data.toString()}"`
        );
        console.debug("StarlingMonkey sent a get-session-port request");
        if (!this._nextSessionPort) {
          console.debug(
            "No debugging session active, telling runtime to continue"
          );
          socket.write("no-session");
        } else {
          console.debug(
            `Starting debug session on port ${this._nextSessionPort}`
          );
          socket.write(`${this._nextSessionPort}\n`);
          this._nextSessionPort = undefined;
        }
      });
      socket.on("close", () => {
        console.debug("ComponentRuntime disconnected");
      });
    }).listen();
  }

  private static serverPort() {
    return (<Net.AddressInfo>this._server!.address()).port;
  }

  private static async ensureHostRuntime(
    config: IStarlingMonkeyRuntimeConfig,
    workspaceFolder: string,
    component: string
  ) {
    if (this._runtimeExecution) {
      return;
    }

    let componentRuntimeArgs = Array.from(config.componentRuntime.options).map(
      (opt) => {
        return opt
          .replace("${workspaceFolder}", workspaceFolder)
          .replace("${component}", component);
      }
    );
    componentRuntimeArgs.push(config.componentRuntime.envOption);
    componentRuntimeArgs.push(
      `STARLINGMONKEY_CONFIG="${config.jsRuntimeOptions.join(" ")}"`
    );
    componentRuntimeArgs.push(config.componentRuntime.envOption);
    componentRuntimeArgs.push(`DEBUGGER_PORT=${this.serverPort()}`);

    console.debug(
      `${config.componentRuntime.executable} ${componentRuntimeArgs.join(" ")}`
    );

    await this.ensureTerminal();

    if (this._terminal!.shellIntegration) {
      this._runtimeExecution = this._terminal!.shellIntegration.executeCommand(
        config.componentRuntime.executable,
        componentRuntimeArgs
      );
      let disposable = window.onDidEndTerminalShellExecution((event) => {
        if (event.execution === this._runtimeExecution) {
          this._runtimeExecution = undefined;
          disposable.dispose();
          console.log(
            `Component host runtime exited with code ${event.exitCode}`
          );
        }
      });
    } else {
      // Fallback to sendText if there is no shell integration.
      // Send Ctrl+C to kill any existing component runtime first.
      this._terminal!.sendText("\x03", false);
      this._terminal!.sendText(
        `${config.componentRuntime.executable} ${componentRuntimeArgs.join(
          " "
        )}`,
        true
      );
    }
  }

  private static async ensureTerminal() {
    if (this._terminal && this._terminal.exitStatus === undefined) {
      return;
    }

    let signal = new Signal<void, void>();
    this._terminal = window.createTerminal();
    let terminalCloseDisposable = window.onDidCloseTerminal((terminal) => {
      if (terminal === this._terminal) {
        signal.resolve();
        this._terminal = undefined;
        this._runtimeExecution = undefined;
        terminalCloseDisposable.dispose();
      }
    });

    let shellIntegrationDisposable = window.onDidChangeTerminalShellIntegration(
      async ({ terminal }) => {
        if (terminal === this._terminal) {
          clearTimeout(timeout);
          shellIntegrationDisposable.dispose();
          signal.resolve();
        }
      }
    );
    // Fallback to sendText if there is no shell integration within 3 seconds of launching
    let timeout = setTimeout(() => {
      shellIntegrationDisposable.dispose();
      signal.resolve();
    }, 3000);

    await signal.wait();
  }
}

export class StarlingMonkeyRuntime extends EventEmitter {
  private _debug!: boolean;
  private _stopOnEntry!: boolean;
  private _sourceMaps!: SourceMaps;
  private _sendingBlocked: boolean = false;
  public get fileAccessor(): FileAccessor {
    return this._fileAccessor;
  }
  public set fileAccessor(value: FileAccessor) {
    this._fileAccessor = value;
  }

  private _server!: Net.Server;
  private _socket!: Net.Socket;

  private _messageReceived = new Signal<DebuggerMessage, void>();

  private _sourceFile!: string;
  public get sourceFile() {
    return this._sourceFile;
  }

  constructor(
    private _workspaceDir: string,
    private _fileAccessor: FileAccessor,
    private _baseConfig: IStarlingMonkeyRuntimeConfig
  ) {
    super();
  }

  public async start(args: ILaunchRequestArguments): Promise<void> {
    let config = applyLaunchArgs(this._baseConfig, args);
    await ComponentRuntimeInstance.start(
      this._workspaceDir,
      config,
      args.component
    );
    this.startSessionServer();
    // TODO: tell StarlingMonkey not to debug if this is false.
    this._debug = args.noDebug ?? true;
    this._stopOnEntry = args.stopOnEntry ?? true;
    this._sourceFile = this.normalizePath(args.program);
    let message = await this._messageReceived.wait();
    assert.equal(
      message.type,
      DebuggerMessageType.Connect,
      `expected "connect" message, got "${message.type}"`
    );
    if (args.trace) {
      this.sendMessage(HostMessageType.StartDebugLogging);
    }
    message = await this.sendAndReceiveMessage(
      HostMessageType.LoadProgram,
      this._sourceFile
    );
    assert.equal(
      message.type,
      DebuggerMessageType.ProgramLoaded,
      `expected "programLoaded" message, got "${message.type}"`
    );
    await this.initSourceMaps(message.source.path!);
    this.emit("programLoaded");
  }

  async initSourceMaps(path: string) {
    path = this.qualifyPath(path);
    this._sourceMaps = new SourceMaps(dirname(path), this._workspaceDir);
  }

  /**
   * Starts a server that creates a new session for every connection request.
   *
   * The server listens for incoming connections and handles data received from the client.
   * It attempts to parse the received data as JSON and resolves the `_messageReceived` promise
   * with the parsed message. If the data cannot be parsed, it is stored in `partialMessage`
   * for the next data event.
   *
   * When the connection ends, the server emits an "end" event.
   *
   * The server listens on a dynamically assigned port, which is then set as the next session port
   * in the `ComponentRuntimeInstance`.
   */
  startSessionServer(): void {
    let debuggerScriptSent = false;
    let partialMessage = "";
    let expectedLength = 0;
    let eol = -1;
    let lengthReceived = false;

    async function resetMessageState() {
      partialMessage = partialMessage.slice(expectedLength);
      expectedLength = 0;
      lengthReceived = false;
      eol = -1;
      if (partialMessage.length > 0) {
        await 1; // Ensure that the current message is processed before the next data event.
        handleData("");
      }
    }

    const handleData = async (data: string) => {
      if (!debuggerScriptSent) {
        if (data.toString() !== "get-debugger") {
          console.warn(
            `expected "get-debugger" message, got "${data.toString()}". Ignoring ...`
          );
          return;
        }
        let extensionDir = `${__dirname}/../`;
        let debuggerScript = await this._fileAccessor.readFile(
          `${extensionDir}/dist/debugger.js`
        );
        this._socket.write(`${debuggerScript.length}\n${debuggerScript}`);
        debuggerScriptSent = true;
        return;
      }

      partialMessage += data;

      if (!lengthReceived) {
        eol = partialMessage.indexOf("\n");
        if (eol === -1) {
          return;
        }
        lengthReceived = true;
        expectedLength = parseInt(partialMessage.slice(0, eol), 10);
        if (isNaN(expectedLength)) {
          console.warn(`expected message length, got "${partialMessage}"`);
          resetMessageState();
          return;
        }
        partialMessage = partialMessage.slice(eol + 1);
        lengthReceived = true;
      }
      if (partialMessage.length < expectedLength) {
        return;
      }
      let message = partialMessage.slice(0, expectedLength);
      try {
        let parsed = JSON.parse(message);
        console.debug(`received message ${partialMessage}`);
        resetMessageState();
        this._messageReceived.resolve(parsed);
      } catch (e) {
        console.warn(
          `Illformed message received. Error: ${e}, message: ${partialMessage}`
        );
        resetMessageState();
      }
    };

    this._server = Net.createServer((socket) => {
      this._socket = socket;
      console.debug("Debug session server accepted connection from client");
      socket.on("data", handleData);
      socket.on("end", () => this.emit("end"));
    }).listen();
    let port = (<Net.AddressInfo>this._server.address()).port;
    ComponentRuntimeInstance.setNextSessionPort(port);
  }

  private sendMessage(type: HostMessageType, value?: any, useRawValue = false) {
    if (this._sendingBlocked) {
      throw new Error("sending blocked");
    }
    let message: string;
    if (useRawValue) {
      message = `{"type": "${type}", "value": ${value}}`;
    } else {
      message = JSON.stringify({ type, value });
    }
    console.debug(`sending message to runtime: ${message}`);
    this._socket.write(`${message.length}\n${message}`);
  }

  private async sendAndReceiveMessage<T extends DebuggerMessage>(
    type: HostMessageType,
    responseType: DebuggerMessageType,
    value?: any,
    useRawValue = false
  ): Promise<T> {
    this.sendMessage(type, value, useRawValue);
    let response = await this.waitForResponse();
    assert.equal(
      response.type,
      responseType,
      `expected "${responseType}" message, got "${response.type}"`
    );
    return response;
  }

  private async waitForResponse(): Promise<any> {
    this._sendingBlocked = true;
    let response = await this._messageReceived.wait();
    this._sendingBlocked = false;
    return response;
  }

  public async run() {
    if (this._debug && this._stopOnEntry) {
      this.emit("stopOnEntry");
    } else {
      this.continue();
    }
  }

  public async continue() {
    // TODO: handle other results, such as run to completion
    await this.sendAndReceiveMessage(HostMessageType.Continue, DebuggerMessageType.StopOnBreakpoint);
    this.emit("stopOnBreakpoint");
  }

  public next(granularity: "statement" | "line" | "instruction") {
    this.handleStep(HostMessageType.Next);
  }

  public stepIn(targetId: number | undefined) {
    this.handleStep(HostMessageType.StepIn);
  }

  public stepOut() {
    this.handleStep(HostMessageType.StepOut);
  }

  private async handleStep(
    type:
      | HostMessageType.Next
      | HostMessageType.StepIn
      | HostMessageType.StepOut
  ) {
    // TODO: handle other results, such as run to completion
    await this.sendAndReceiveMessage(type, DebuggerMessageType.StopOnStep);
    this.emit("stopOnStep");
  }

  public async stack(index: number, count: number): Promise<DebugProtocol.StackFrame[]> {
    let message = await this.sendAndReceiveMessage<DebuggerMessageStack>(HostMessageType.GetStack, DebuggerMessageType.Stack, {
      index,
      count,
    });
    let stack = message.stack;
    for (let frame of stack) {
      await this._translateLocationFromContent(frame);
      frame.source!.path = this.qualifyPath(frame.source!.path!);
    }
    return stack;
  }

  private async _translateLocationFromContent(frame: any) {
    if (typeof frame.column === "number" && frame.column > 0) {
      frame.column -= 1;
    }
    if (!this._sourceMaps) {
      return true;
    }
    return await this._sourceMaps.MapToSource(frame);
  }

  private async _translateLocationToContent(frame: any) {
    if (this._sourceMaps) {
      await this._sourceMaps.MapFromSource(frame);
    }
    if (typeof frame.column === "number") {
      frame.column += 1;
    }
  }

  async getScopes(frameId: number): Promise<Scope[]> {
    let message = await this.sendAndReceiveMessage(
      HostMessageType.GetScopes,
      frameId
    );
    assert.equal(
      message.type,
      DebuggerMessageType.Scopes,
      `expected "scopes" message, got "${message.type}"`
    );
    return message.value;
  }

  public async getBreakpointLocations(
    path: string,
    line: number
  ): Promise<{ line: number; column: number }[]> {
    while (this._sendingBlocked) {
      await this._messageReceived.wait();
    }
    // TODO: support the full set of query params from BreakpointLocationsArguments
    path = this.normalizePath(path);
    let loc = new SourceLocation(path, line, 0);
    await this._translateLocationToContent(loc);

    let message = await this.sendAndReceiveMessage(
      HostMessageType.GetBreakpointsForLine,
      loc
    );
    assert.equal(
      message.type,
      DebuggerMessageType.BreakpointsForLine,
      `expected "breakpointsForLine" message, got "${message.type}"`
    );
    return message.value;
  }

  public async setBreakPoint(
    path: string,
    line: number,
    column?: number
  ): Promise<IRuntimeBreakpoint> {
    path = this.normalizePath(path);
    let loc = new SourceLocation(path, line, column ?? 0);
    await this._translateLocationToContent(loc);

    let response = await this.sendAndReceiveMessage(
      HostMessageType.SetBreakpoint,
      loc
    );
    assert.equal(
      response.type,
      "breakpointSet",
      `expected "breakpointSet" message, got "${response.type}"`
    );

    let bp = response.value;
    if (bp.id !== -1) {
      await this._translateLocationFromContent(bp);
    }
    return bp;
  }

  public async getVariables(reference: number): Promise<IRuntimeVariable[]> {
    let message = await this.sendAndReceiveMessage(
      HostMessageType.GetVariables,
      reference
    );
    assert.equal(
      message.type,
      DebuggerMessageType.Variables,
      `expected "variables" message, got "${message.type}"`
    );
    return message.value;
  }

  public async setVariable(
    variablesReference: number,
    name: string,
    value: string
  ): Promise<IRuntimeVariable> {
    // Manually encode the value so that it'll be decoded as raw values by the runtime, instead of everything becoming a string.
    let rawValue = `{"variablesReference": ${variablesReference}, "name": "${name}", "value": ${value}}`;
    let message = await this.sendAndReceiveMessage(
      HostMessageType.SetVariable,
      rawValue,
      true
    );
    assert.equal(
      message.type,
      DebuggerMessageType.VariableSet,
      `expected "variableSet" message, got "${message.type}"`
    );
    return message.value;
  }

  private normalizePath(path: string) {
    path = path.replace(/\\/g, "/");
    return path.startsWith(this._workspaceDir)
      ? path.substring(this._workspaceDir.length + 1)
      : path;
  }

  private qualifyPath(path: string) {
    return `${this._workspaceDir}/${path}`;
  }
}
function applyLaunchArgs(baseConfig: IStarlingMonkeyRuntimeConfig, args: ILaunchRequestArguments) {
  let config = {
    componentRuntime: {
      executable: args["componentRuntime.executable"] ?? baseConfig.componentRuntime.executable,
      options: args["componentRuntime.options"] ?? baseConfig.componentRuntime.options,
      envOption: args["componentRuntime.envOption"] ?? baseConfig.componentRuntime.envOption,
    },
    jsRuntimeOptions: args.jsRuntimeOptions ?? baseConfig.jsRuntimeOptions,
    trace: args.trace ?? baseConfig.trace,
   };

  return config;
}
