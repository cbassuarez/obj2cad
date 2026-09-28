import { createTheme, type MantineColorsTuple } from "@mantine/core";

// Mantine reads the same CSS variables as Tailwind and the shadcn components, so a theme
// switch (a class on <html>) restyles everything at once.
const token = (name: string): MantineColorsTuple =>
  Array.from({ length: 10 }, () => `var(${name})`) as unknown as MantineColorsTuple;

export const mantineTheme = createTheme({
  fontFamily: "var(--font-sans)",
  fontFamilyMonospace: "var(--font-mono)",
  headings: { fontFamily: "var(--font-display)", fontWeight: "600" },
  primaryColor: "accent",
  colors: { accent: token("--accent") },
  defaultRadius: "sm",
  radius: { xs: "2px", sm: "3px", md: "4px", lg: "4px", xl: "4px" },
  cursorType: "pointer",
  focusRing: "never", // our own :focus-visible ring applies everywhere
});
