/** @type {import('tailwindcss').Config} */
export default {
  content: ["./src/**/*.{html,js,svelte,ts}", "./index.html"],
  theme: {
    extend: {
      colors: {
        // shadcn-svelte zinc dark palette (adjusted for better card contrast)
        background: "#09090b",
        foreground: "#fafafa",
        card: "#1a1a1d",
        "card-hover": "#222225",
        muted: "#27272a",
        "muted-foreground": "#a1a1aa",
        border: "#27272a",
      },
    },
  },
  plugins: [],
};
