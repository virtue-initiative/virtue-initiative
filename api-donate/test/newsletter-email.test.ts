import { describe, expect, it } from 'vitest';
import { markdownToText, newsletterFooter } from '../src/lib/newsletter-email';

describe('markdownToText', () => {
  it('keeps link targets and drops markdown markers', () => {
    const text = markdownToText(
      [
        '## Who are we?',
        'You can [install it](https://virtueinitiative.org/download) now.',
        '1. **[Donate](https://virtueinitiative.org/donate)**. Please *consider* it.',
        'Write to [help@virtueinitiative.org](mailto:help@virtueinitiative.org) or `ask`.',
        'Andrew  ',
      ].join('\n'),
    );
    expect(text).toBe(
      [
        'Who are we?',
        'You can install it (https://virtueinitiative.org/download) now.',
        '1. Donate (https://virtueinitiative.org/donate). Please consider it.',
        'Write to help@virtueinitiative.org or ask.',
        'Andrew',
      ].join('\n'),
    );
  });

  it('leaves list bullets alone', () => {
    expect(markdownToText('* one\n* two')).toBe('* one\n* two');
  });
});

describe('newsletterFooter', () => {
  it('includes the postal address and unsubscribe link', () => {
    const footer = newsletterFooter('https://x.test/u?token=t', '75 Knapp Rd, Mechanicville, NY');
    expect(footer.text).toContain('75 Knapp Rd, Mechanicville, NY');
    expect(footer.html).toContain('href="https://x.test/u?token=t"');
  });
});
