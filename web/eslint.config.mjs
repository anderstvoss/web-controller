import security from "eslint-plugin-security";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "frontend-bom.json"] },
  ...tseslint.configs.strict,
  security.configs.recommended,
  {
    rules: {
      "no-eval": "error",
      "no-new-func": "error",
      "no-implied-eval": "error",
    },
  },
);
