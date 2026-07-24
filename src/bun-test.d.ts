declare module "bun:test" {
  type TestFn = () => void | Promise<void>;

  interface Matchers {
    toBe(expected: unknown): void;
    toContain(expected: unknown): void;
    toEqual(expected: unknown): void;
    toMatchObject(expected: Record<string, unknown>): void;
    toHaveLength(expected: number): void;
    toBeUndefined(): void;
    toBeNull(): void;
    toBeTruthy(): void;
    toBeGreaterThan(expected: number): void;
    toBeGreaterThanOrEqual(expected: number): void;
    toMatch(expected: string | RegExp): void;
  }

  interface AsyncMatchers {
    toBe(expected: unknown): Promise<void>;
  }

  interface Expectation extends Matchers {
    not: Pick<Matchers, "toContain" | "toBe" | "toBeNull">;
    resolves: AsyncMatchers;
  }

  export function describe(name: string, fn: TestFn): void;
  export function test(name: string, fn: TestFn): void;
  /** Alias for `test`. */
  export function it(name: string, fn: TestFn): void;
  export function beforeEach(fn: TestFn): void;
  export function afterEach(fn: TestFn): void;
  export function expect(actual: unknown): Expectation;
}

declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8" | "utf-8"): string;
}
