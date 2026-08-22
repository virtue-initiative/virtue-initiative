import { z } from 'zod';

// Parses a multipart form field that carries a JSON string (e.g. `metadata`), surfacing
// both malformed JSON and schema violations through the same zod issue-reporting path
// as every other validateZ() failure.
export function jsonField<Schema extends z.ZodTypeAny>(schema: Schema, label: string) {
  return z.string().transform((raw, ctx) => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      ctx.addIssue({ code: 'custom', message: `${label} must be valid JSON` });
      return z.NEVER;
    }

    const result = schema.safeParse(parsed);
    if (!result.success) {
      for (const issue of result.error.issues) {
        ctx.addIssue({ code: 'custom', message: issue.message, path: issue.path });
      }
      return z.NEVER;
    }

    return result.data as z.infer<Schema>;
  });
}
