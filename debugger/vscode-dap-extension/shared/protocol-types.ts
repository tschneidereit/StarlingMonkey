export interface IRuntimeStackFrame {
  index: number;
  name?: string;
  path?: string;
  line?: number;
  column?: number;
  instruction?: number;
}

export interface DebuggerMessage {
  type: DebuggerMessageType;
  value?: any;
}

export enum DebuggerMessageType {
  Connect = "connect",
  ProgramLoaded = "programLoaded",
  BreakpointSet = "breakpointSet",
  StopOnStep = "stopOnStep",
  StopOnBreakpoint = "stopOnBreakpoint",
  BreakpointsForLine = "breakpointsForLine",
  Stack = "stack",
  Scopes = "scopes",
  Variables = "variables",
  VariableSet = "variableSet",
}

export interface HostMessage {
  type: HostMessageType;
  value?: any;
}

export enum HostMessageType {
  StartDebugLogging = "startDebugLogging",
  StopDebugLogging = "stopDebugLogging",
  LoadProgram = "loadProgram",
  Continue = "continue",
  Next = "next",
  StepIn = "stepIn",
  StepOut = "stepOut",
  SetBreakpoint = "setBreakpoint",
  RemoveBreakpoint = "removeBreakpoint",
  GetStack = "getStack",
  GetScopes = "getScopes",
  GetVariables = "getVariables",
  SetVariable = "setVariable",
  GetBreakpointsForLine = "getBreakpointsForLine",
}
