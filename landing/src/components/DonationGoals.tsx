import { useEffect, useState } from 'preact/hooks';
import { DONATE_API_URL } from '../lib/donate-url';

interface Goal {
  id: string;
  title: string;
  description: string | null;
  targetCents: number;
  raisedCents: number;
}

function formatDollars(cents: number): string {
  return `$${Math.round(cents / 100).toLocaleString('en-US')}`;
}

export function DonationGoals() {
  const [goals, setGoals] = useState<Goal[]>([]);

  useEffect(() => {
    fetch(`${DONATE_API_URL}/goals`)
      .then((response) => (response.ok ? response.json() : Promise.reject(response.status)))
      .then((data: { goals: Goal[] }) => setGoals(data.goals))
      .catch((err) => console.error('Failed to load donation goals', err));
  }, []);

  if (goals.length === 0) return null;

  return (
    <section class="donate-goals" aria-labelledby="donate-goals-heading">
      <h3 id="donate-goals-heading">
        {goals.length === 1 ? 'Our fundraising goal' : 'Our fundraising goals'}
      </h3>
      <p class="donate-goals-intro">
        {goals.length === 1
          ? 'This is the goal we are fundraising for right now.'
          : 'These are the goals we are fundraising for right now.'}
      </p>
      {goals.map((goal) => {
        const percent = Math.min(100, Math.round((goal.raisedCents / goal.targetCents) * 100));
        return (
          <div class="donate-goal" key={goal.id}>
            <div class="donate-goal-header">
              <h4>{goal.title}</h4>
              <span>
                {formatDollars(goal.raisedCents)} of {formatDollars(goal.targetCents)}
              </span>
            </div>
            <div
              class="donate-goal-bar"
              role="progressbar"
              aria-label={goal.title}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent}
            >
              <div class="donate-goal-fill" style={{ width: `${percent}%` }} />
            </div>
            {goal.description && <p class="donate-goal-description">{goal.description}</p>}
          </div>
        );
      })}
    </section>
  );
}
