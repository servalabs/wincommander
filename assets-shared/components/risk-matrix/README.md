# RiskMatrix Integration Notes

`RiskMatrix` is a shared React component consumed by `web/website-current`
through the `@assets` Vite alias. Because it lives outside the app package,
its bare imports for `react`, `react-dom`, `framer-motion`, and `lucide-react`
must resolve to the website app's own dependency graph.

The required Vite setting lives in `web/website-current/vite.config.ts`:

```ts
resolve: {
  alias: {
    "@": path.resolve(__dirname, "./src"),
    "@assets": path.resolve(__dirname, "../../assets"),
  },
  dedupe: ["react", "react-dom", "framer-motion", "lucide-react"],
}
```

Do not remove the `dedupe` list when moving the component or changing package
managers. Without it, Vite can resolve a second React instance for files under
`assets/components/risk-matrix`, which breaks hooks at runtime.

Verification used for the website integration:

- `npm run test -- src/pages/new/NetWall.test.tsx src/pages/new/SolutionsDefense.test.tsx src/pages/new/SolutionsEnterprise.test.tsx src/pages/new/SolutionsIntelligence.test.tsx src/pages/new/Newsroom.test.tsx src/pages/InvestPage.test.tsx src/pages/Index.test.tsx src/pages/new/Solutions.test.tsx src/pages/new/SolutionsLawEnforcement.test.tsx`
- `npm run build`
- Inspect `dist/assets/index-*.js` after build: React is bundled once through the app entry, and no separate RiskMatrix-owned React chunk is emitted.
