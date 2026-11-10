// A plain object on purpose: importing 'vitest/config' only resolves inside the package root, and this file is run from the repo root.
export default {
  resolve: { alias: { '@shared': `${process.cwd()}/src/shared` } },
  test: {
    include: ['rewrite-prep/fixtures/_generator/gen-fixtures.test.ts'],
    environment: 'node',
    testTimeout: 120_000,
  },
};
