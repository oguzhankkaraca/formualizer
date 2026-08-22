declare module 'formualizer-wasm-init' {
  const initializeWasm: () => Promise<unknown>;
  export const Workbook: unknown;
  export default initializeWasm;
}
