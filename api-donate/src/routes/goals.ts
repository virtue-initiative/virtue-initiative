import { Hono } from 'hono';
import { getActiveGoals } from '../lib/db';
import { Env, Variables } from '../types/bindings';

const goals = new Hono<{ Bindings: Env; Variables: Variables }>();

goals.get('/goals', async (c) => {
  return c.json({ goals: await getActiveGoals(c.env.DB) });
});

export default goals;
