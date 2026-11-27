let instance = 0;

export async function loadSdk() {
  // A distinct module URL creates fresh binding state and a new WASM instance.
  const url = `/generated/bindings/kohaku_tornadocash_wasm.js?instance=${instance++}`;
  const sdk = await import(/* @vite-ignore */ url);
  await sdk.default();
  return sdk;
}

export async function loadArtifactBytes() {
  const response = await fetch('/generated/artifacts.bin');
  if (!response.ok) throw new Error(`Cannot load benchmark artifacts: ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}
