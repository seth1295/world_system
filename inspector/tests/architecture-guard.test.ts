import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { describe, expect, it } from 'vitest';

const here = dirname(fileURLToPath(import.meta.url));
const sourceRoot = join(here, '..', 'src');

// Keep the semantic vocabulary in one place so the source scan and reviewer share one rule set.
const forbiddenSemanticTerms: readonly RegExp[] = [
  /\bterrain\b/i,
  /\bocean\b/i,
  /\belevation\b/i,
  /\bclimate\b/i,
  /\btectonics?\b/i,
  /\becology\b/i,
  /\bseasons?\b/i,
  /\bhemisphere\b/i,
  /\bstellar\b/i,
  /\bstar\b/i,
  /\btopography\b/i,
  /\bhydrology\b/i,
  /\brock\b/i,
  /\birregular\b/i,
  /\bcell[ _-]*key\b/i,
  /\bprojection\b/i,
  /\bsampling\b/i,
  /\binterpolation\b/i,
  /\brefinement\b/i,
  /\bderived[- ]field\b/i,
  /\bfeature[- ]resolution\b/i,
  /\borbital[- ]propagation\b/i,
  /\.veyra\b/i,
];

describe('source architecture guards', () => {
  it('keeps world semantics out of UI and rendering source', () => {
    const files = sourceFiles(sourceRoot).filter((file) => !file.includes(`${join('src', 'fixtures')}`) && !file.includes(`${join('src', 'provider')}`));
    const violations: string[] = [];
    for (const file of files) {
      const content = readFileSync(file, 'utf8');
      for (const pattern of forbiddenSemanticTerms) {
        if (pattern.test(content)) violations.push(`${relative(sourceRoot, file)}: ${pattern.source}`);
      }
    }
    expect(violations).toEqual([]);
  });

  it('keeps fixture and concrete mock imports outside the UI and renderer', () => {
    const files = [...sourceFiles(join(sourceRoot, 'ui')), ...sourceFiles(join(sourceRoot, 'render'))].filter((file) => file.endsWith('.ts') || file.endsWith('.tsx'));
    const violations: string[] = [];
    for (const file of files) {
      const source = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true);
      for (const statement of source.statements) {
        if (!ts.isImportDeclaration(statement) || !ts.isStringLiteral(statement.moduleSpecifier)) continue;
        const imported = statement.moduleSpecifier.text;
        if (/fixtures|provider\/mock-provider/.test(imported)) violations.push(`${relative(sourceRoot, file)} imports ${imported}`);
      }
    }
    expect(violations).toEqual([]);
    const app = readFileSync(join(sourceRoot, 'ui', 'app.ts'), 'utf8');
    expect(app).toContain('bodyCatalog: BodyCatalog');
    expect(app).toContain('provider: BodyProvider | null');
  });

});

function sourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path);
    return entry.isFile() && /\.(ts|tsx|css)$/.test(entry.name) ? [path] : [];
  });
}
