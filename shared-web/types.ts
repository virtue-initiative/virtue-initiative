import { z } from 'zod';

export const emailFrequencySchema = z.enum(['none', 'alerts-only', 'daily', 'weekly']);
export type EmailFrequency = z.infer<typeof emailFrequencySchema>;
export const emailFrequencies = emailFrequencySchema.options;

export const hashParamsSchema = z.object({
  version: z.string(),
  algorithm: z.string(),
  memory_cost_kib: z.number(),
  time_cost: z.number(),
  parallelism: z.number(),
  salt_length: z.number(),
  hkdf_hash: z.string(),
});
export type HashParams = z.infer<typeof hashParamsSchema>;

export const loginMaterialSchema = z.object({
  password_salt: z.string().optional(),
  params: hashParamsSchema,
});
export type LoginMaterial = z.infer<typeof loginMaterialSchema>;

export const userSchema = z.object({
  id: z.string(),
  email: z.string(),
  email_verified: z.boolean(),
  email_bounced_at: z.number().nullable(),
  settings: z.object({
    email_frequency: emailFrequencySchema,
    timezone: z.string(),
  }),
  name: z.string().optional(),
  pub_key: z.string().optional(),
  encrypted_priv_key: z.string().optional(),
});
export type User = z.infer<typeof userSchema>;

export const deviceSchema = z.object({
  id: z.string(),
  owner: z.string(),
  name: z.string(),
  platform: z.string(),
  last_upload_at: z.number().nullable(),
  last_hash_at: z.number().nullable(),
  pending_count: z.number(),
  status: z.enum(['online', 'offline', 'logged_out']),
});
export type Device = z.infer<typeof deviceSchema>;

export const batchSchema = z.object({
  id: z.string(),
  device_id: z.string(),
  start_time: z.number(),
  end_time: z.number(),
  end_hash: z.string(),
  version: z.string(),
  url: z.string(),
  encrypted_key: z.string(),
  created_at: z.number(),
});
export type Batch = z.infer<typeof batchSchema>;

export const dataLogSchema = z.object({
  id: z.string(),
  device_id: z.string(),
  ts: z.number(),
  type: z.string(),
  data: z.record(z.string(), z.unknown()),
  created_at: z.number(),
  risk: z.number().optional(),
});
export type DataLog = z.infer<typeof dataLogSchema>;

const partnerUserSchema = z.object({
  id: z.string(),
  email: z.string(),
  name: z.string().optional(),
});

export const partnerInfoSchema = z.object({
  id: z.string(),
  user: partnerUserSchema.extend({ id: z.string().optional() }),
  status: z.enum(['pending', 'accepted']),
  created_at: z.number().optional(),
});
export type PartnerInfo = z.infer<typeof partnerInfoSchema>;

export const watchingPartnerSchema = partnerInfoSchema;
export type WatchingPartner = PartnerInfo;

export const watcherPartnerSchema = partnerInfoSchema;
export type WatcherPartner = PartnerInfo;

export const partnerRelationshipsSchema = z.object({
  watching: z.array(watchingPartnerSchema),
  watchers: z.array(watcherPartnerSchema),
});
export type PartnerRelationships = z.infer<typeof partnerRelationshipsSchema>;

export const dataPageSchema = z.object({
  batches: z.array(batchSchema),
  user: userSchema,
  watching: z.array(partnerInfoSchema),
  watchers: z.array(partnerInfoSchema),
});
export type DataPage = z.infer<typeof dataPageSchema>;

export const partnerInviteValidationSchema = z.object({
  ok: z.boolean(),
  partnership_id: z.string(),
  owner: partnerUserSchema,
});
export type PartnerInviteValidation = z.infer<typeof partnerInviteValidationSchema>;

export const passwordResetValidationSchema = z.object({
  ok: z.boolean(),
  email: z.string(),
});
export type PasswordResetValidation = z.infer<typeof passwordResetValidationSchema>;

export const signupValidationSchema = z.object({
  email: z.string(),
});
export type SignupValidation = z.infer<typeof signupValidationSchema>;

// ── Request schemas ──────────────────────────────────────────────────────────

export const signupRequestSchema = z.object({
  email: z.email(),
  to: z.string().optional(),
});
export type SignupRequest = z.infer<typeof signupRequestSchema>;

export const signupSchema = z.object({
  verification_token: z.string().min(1),
  password_auth: z.base64(),
  password_salt: z.base64(),
  pub_key: z.base64(),
  encrypted_priv_key: z.base64(),
  name: z.string().min(1).optional(),
});
export type SignupPayload = z.infer<typeof signupSchema>;

export const signupValidateSchema = z.object({ token: z.string().min(1) });
export type SignupValidatePayload = z.infer<typeof signupValidateSchema>;

export const loginMaterialQuerySchema = z.object({ email: z.email().optional() });
export type LoginMaterialQuery = z.infer<typeof loginMaterialQuerySchema>;

export const loginSchema = z.object({
  email: z.email(),
  password_auth: z.base64(),
  timezone: z.string().optional(),
});
export type LoginPayload = z.infer<typeof loginSchema>;

export const verifyEmailSchema = z.object({ token: z.string().min(1) });
export type VerifyEmailPayload = z.infer<typeof verifyEmailSchema>;

export const passwordResetRequestSchema = z.object({ email: z.email() });
export type PasswordResetRequest = z.infer<typeof passwordResetRequestSchema>;

export const passwordResetValidateSchema = z.object({ token: z.string().min(1) });
export type PasswordResetValidatePayload = z.infer<typeof passwordResetValidateSchema>;

export const passwordResetSchema = z.object({
  token: z.string().min(1),
  password_auth: z.base64(),
  password_salt: z.base64(),
  pub_key: z.base64(),
  encrypted_priv_key: z.base64(),
});
export type PasswordResetPayload = z.infer<typeof passwordResetSchema>;

export const updateUserSchema = z
  .object({
    email: z.email().optional(),
    name: z.string().min(1).optional(),
    settings: z
      .object({
        email_frequency: emailFrequencySchema.optional(),
        timezone: z.string().optional(),
      })
      .optional(),
    pub_key: z.base64().optional(),
    encrypted_priv_key: z.base64().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });
export type UpdateUserPayload = z.infer<typeof updateUserSchema>;

export const deleteUserSchema = z.object({ confirm_email: z.email() });
export type DeleteUserPayload = z.infer<typeof deleteUserSchema>;

export const createPartnerSchema = z.object({ email: z.email() });
export type CreatePartnerPayload = z.infer<typeof createPartnerSchema>;

export const inviteTokenSchema = z.object({ token: z.string().min(1) });
export type InviteTokenPayload = z.infer<typeof inviteTokenSchema>;

export const updateDeviceSchema = z
  .object({ name: z.string().min(1).optional() })
  .refine((data) => Object.keys(data).length > 0, { message: 'No fields to update' });
export type UpdateDevicePayload = z.infer<typeof updateDeviceSchema>;

// ── Additional response schemas ──────────────────────────────────────────────

export const signupResponseSchema = z.object({
  user: z.object({
    id: z.string(),
    email: z.string(),
    email_verified: z.boolean(),
    name: z.string().optional(),
  }),
});
export type SignupResponse = z.infer<typeof signupResponseSchema>;

export const emailVerifyResponseSchema = z.object({
  ok: z.boolean(),
  email: z.string(),
  purpose: z.enum(['email_change', 'email_verification']),
});
export type EmailVerifyResponse = z.infer<typeof emailVerifyResponseSchema>;

export const updateUserResponseSchema = z.object({
  ok: z.boolean(),
  email_verification_required: z.boolean().optional(),
  pending_email: z.string().optional(),
});
export type UpdateUserResponse = z.infer<typeof updateUserResponseSchema>;

export const createPartnerResponseSchema = z.object({
  id: z.string(),
  status: z.literal('pending'),
});
export type CreatePartnerResponse = z.infer<typeof createPartnerResponseSchema>;
