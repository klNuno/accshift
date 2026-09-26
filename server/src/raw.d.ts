// Vite's `?raw` suffix imports a file as a string. Only tests use it.
declare module "*?raw" {
  const content: string;
  export default content;
}
