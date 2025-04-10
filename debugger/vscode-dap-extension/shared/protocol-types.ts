import { DebugProtocol } from "@vscode/debugprotocol";

export interface SourceLocation {
  line: number;
  column: number;
}

export type DebuggerMessage =
    DebuggerMessageConnect
  | DebuggerMessageProgramLoaded
  | DebuggerMessageBreakpointSet
  | DebuggerMessageStopOnBreakpoint
  | DebuggerMessageStopOnStep
  | DebuggerMessageBreakpointsForLine
  | DebuggerMessageStack
  | DebuggerMessageScopes
  | DebuggerMessageVariables
  | DebuggerMessageVariableSet;

export interface DebuggerMessageConnect {
  type: DebuggerMessageType.Connect;
}

export interface DebuggerMessageProgramLoaded {
  type: DebuggerMessageType.ProgramLoaded;
  source: DebugProtocol.Source;
}

export interface DebuggerMessageBreakpointsForLine {
  type: DebuggerMessageType.BreakpointsForLine;
  locations: SourceLocation[];
}

export interface DebuggerMessageBreakpointSet {
  type: DebuggerMessageType.BreakpointSet;
  id: number;
  location: SourceLocation;
}

export interface DebuggerMessageStack {
  type: DebuggerMessageType.Stack;
  stack: DebugProtocol.StackFrame[];
}

export interface DebuggerMessageScopes {
  type: DebuggerMessageType.Scopes;
  scopes: DebugProtocol.Scope[];
}

export interface DebuggerMessageVariables {
  type: DebuggerMessageType.Variables;
  variables: DebugProtocol.Variable[];
}

export interface DebuggerMessageVariableSet {
  type: DebuggerMessageType.VariableSet;
  newValue: any;
}

export interface DebuggerMessageStopOnBreakpoint {
  type: DebuggerMessageType.StopOnBreakpoint;
  id: number;
}

export interface DebuggerMessageStopOnStep {
  type: DebuggerMessageType.StopOnStep;
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
