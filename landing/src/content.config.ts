import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';

const blog = defineCollection({
  loader: glob({ pattern: '**/*.{md,mdx}', base: './src/content/blog' }),
  schema: z.object({
    title: z.string(),
    description: z.string(),
    // Fun trick to set time to 00:00:00, so that the date is actually correct
    pubDate: z.coerce.date().transform((date) => new Date(date.toISOString().split('T')[0] + ' ')),
    author: z.string().optional(),
    draft: z.boolean().default(false),
  }),
});

export const collections = { blog };
