/** Mirrors the core's skin check so a wrong file fails before any upload. */
const PNG_SIGNATURE = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
const MAX_SKIN_BYTES = 256 * 1024;

export interface SkinFile {
  base64: string;
  /** For the 3D preview; the CSP allows data URLs only. */
  dataUrl: string;
}

/** The PNG as base64, or `null` when it is not a 64x64 or 64x32 PNG. */
export async function readSkinFile(file: Blob): Promise<SkinFile | null> {
  if (file.size > MAX_SKIN_BYTES) return null;
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (bytes.length < 24 || PNG_SIGNATURE.some((byte, at) => bytes[at] !== byte)) return null;
  if (String.fromCharCode(...bytes.subarray(12, 16)) !== "IHDR") return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = view.getUint32(16);
  const height = view.getUint32(20);
  if (width !== 64 || (height !== 64 && height !== 32)) return null;
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  const base64 = btoa(binary);
  return { base64, dataUrl: `data:image/png;base64,${base64}` };
}
