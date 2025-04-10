
// Type definitions for SpiderMonkey Debugger API
export declare class Debugger {
  static Object: any;
  static Script: any;
  static Environment: any;
  static Frame: any;
  constructor();
  addAllGlobalsAsDebuggees(): void;
  findScripts(): Debugger.Script[];
  onNewScript: ((script: Debugger.Script, global?: any) => void) | undefined;
  onEnterFrame: ((frame: Debugger.Frame) => void) | undefined;
}

export declare namespace Debugger {
  interface Script {
    url: string;
    startLine: number;
    startColumn: number;
    lineCount: number;
    global: Object;
    getOffsetMetadata(offset: number): {
      lineNumber: number;
      columnNumber: number;
    };
    getPossibleBreakpointOffsets(options: { line: number, minColumn?: number }): number[];
    getChildScripts(): Script[];
    setBreakpoint(offset: number, handler: BreakpointHandler): void;
  }

  interface Frame {
    script: Script;
    offset: number;
    this?: Object;
    type: string;
    older?: Frame;
    olderSavedFrame?: Frame;
    callee?: { name: string };
    environment: Environment;
    onStep: (() => void) | undefined;
    onPop: (() => void) | undefined;
  }

  interface Environment {
    names(): string[];
    getVariable(name: string): any;
    setVariable(name: string, value: any): void;
  }

  interface Object {
    class?: string;
    getOwnPropertyNames(): string[];
    getOwnPropertyDescriptor(name: string): PropertyDescriptor;
    setProperty(name: string, value: any): void;
  }

  interface PropertyDescriptor {
    value?: any;
    get?: Object;
    set?: Object;
  }

  interface BreakpointHandler {
    hit(frame: Frame): void;
  }
}
