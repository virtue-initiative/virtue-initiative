import { Button } from '@virtueinitiative/shared-web';
import { useState } from 'preact/hooks';
import { DONATE_API_URL, STRIPE_PORTAL_URL } from '../lib/donate-url';

const PRESET_AMOUNTS = [5, 10, 25, 50, 100];

export function DonateForm() {
  const [amount, setAmount] = useState<number>(25);
  const [customAmount, setCustomAmount] = useState<string>('');
  const [recurring, setRecurring] = useState<boolean>(false);
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  // The custom input, when filled, overrides the selected preset.
  const effectiveAmount = customAmount.trim() !== '' ? Number(customAmount) : amount;
  const amountIsValid = Number.isFinite(effectiveAmount) && effectiveAmount >= 1;

  function selectPreset(value: number) {
    setAmount(value);
    setCustomAmount('');
  }

  async function handleSubmit(event: Event) {
    event.preventDefault();
    setError(null);

    if (!amountIsValid) {
      setError('Please enter a donation amount of at least $1.');
      return;
    }

    setSubmitting(true);
    try {
      const response = await fetch(`${DONATE_API_URL}/checkout`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ amount: effectiveAmount, recurring }),
      });

      if (!response.ok) {
        throw new Error(`Checkout failed (${response.status})`);
      }

      const { url } = (await response.json()) as { url: string };
      window.location.href = url;
    } catch (err) {
      console.error(err);
      setError('Something went wrong starting your donation. Please try again.');
      setSubmitting(false);
    }
  }

  return (
    <form class="donate-form" onSubmit={handleSubmit}>
      <fieldset class="donate-frequency" disabled={submitting}>
        <legend>Donation type</legend>
        <div class="donate-toggle">
          <button
            type="button"
            class={`donate-toggle-option${recurring ? '' : ' is-active'}`}
            aria-pressed={!recurring}
            onClick={() => setRecurring(false)}
          >
            One-time
          </button>
          <button
            type="button"
            class={`donate-toggle-option${recurring ? ' is-active' : ''}`}
            aria-pressed={recurring}
            onClick={() => setRecurring(true)}
          >
            Monthly
          </button>
        </div>
      </fieldset>

      <fieldset class="donate-amounts" disabled={submitting}>
        <legend>Amount</legend>
        <div class="donate-preset-grid">
          {PRESET_AMOUNTS.map((value) => (
            <button
              type="button"
              key={value}
              class={`donate-preset${customAmount.trim() === '' && amount === value ? ' is-active' : ''}`}
              aria-pressed={customAmount.trim() === '' && amount === value}
              onClick={() => selectPreset(value)}
            >
              ${value}
            </button>
          ))}
        </div>
        <label class="donate-custom">
          <span>Custom amount</span>
          <div class="donate-custom-input">
            <span class="donate-currency">$</span>
            <input
              type="number"
              min="1"
              step="1"
              inputMode="decimal"
              placeholder="Other"
              value={customAmount}
              onInput={(event) => setCustomAmount((event.target as HTMLInputElement).value)}
            />
          </div>
        </label>
      </fieldset>

      {error && <p class="donate-error">{error}</p>}

      <Button type="submit" variant="primary" disabled={submitting || !amountIsValid}>
        {submitting
          ? 'Redirecting to checkout…'
          : recurring
            ? `Donate $${amountIsValid ? effectiveAmount : ''} monthly`
            : `Donate $${amountIsValid ? effectiveAmount : ''}`}
      </Button>

      <p class="donate-secure-note">
        You'll be securely redirected to Stripe to complete your gift.
      </p>

      {STRIPE_PORTAL_URL && (
        <div class="donate-manage">
          <p>Already a recurring donor?</p>
          <Button href={STRIPE_PORTAL_URL} variant="outline" size="sm">
            Manage donations
          </Button>
        </div>
      )}
    </form>
  );
}
