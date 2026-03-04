import { z } from 'zod';

// Auth schemas
export const signupSchema = z.object({
  email: z.email(),
  password: z.string().min(8),
  name: z.string().optional(),
});

export const loginSchema = z.object({
  email: z.email(),
  password: z.string().min(1),
});

// Device schemas
export const createDeviceSchema = z.object({
  name: z.string().min(1),
  platform: z.string().min(1),
});

export const listDevicesSchema = z.object({
  user: z.string().min(1).optional(),
});

export const updateDeviceSchema = z
  .object({
    name: z.string().min(1).optional(),
    enabled: z.boolean().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, {
    message: 'No fields to update',
  });

// Batch schemas
export const uploadBatchSchema = z.object({
  device_id: z.string().min(1),
  start_time: z.iso.datetime(),
  end_time: z.iso.datetime(),
  item_count: z.coerce.number().int().nonnegative(),
  size_bytes: z.coerce.number().int().nonnegative(),
});

export const listBatchesSchema = z.object({
  device_id: z.string().min(1).optional(),
  user: z.string().optional(),
  cursor: z.string().optional(),
  limit: z.coerce.number().int().positive().max(100).optional().default(50),
});

// Hash chain schemas
export const getStateSchema = z.object({
  device_id: z.string().min(1),
  user: z.string().optional(),
});

// Partner schemas
export const createPartnerSchema = z.object({
  email: z.email(),
  permissions: z
    .object({
      view_data: z.boolean().optional().default(true),
    })
    .optional()
    .default({ view_data: true }),
});

export const acceptPartnerSchema = z.object({
  id: z.string().min(1),
  encryptedE2EEKey: z.string().optional(),
});

export const updatePartnerSchema = z.object({
  permissions: z.object({
    view_data: z.boolean().optional(),
  }),
});

// E2EE key schemas
export const setE2EEKeySchema = z.object({
  encryptedE2EEKey: z.string().min(1),
});

// User profile schemas
export const updateMeSchema = z
  .object({ name: z.string().optional() })
  .refine((d) => Object.keys(d).length > 0, { message: 'No fields to update' });
export const settingsSchema = z.object({
  name: z.string().optional(),
  timezone: z.string().optional(),
  retention_days: z.number().int().positive().optional(),
});

export type SignupInput = z.infer<typeof signupSchema>;
export type LoginInput = z.infer<typeof loginSchema>;
export type CreateDeviceInput = z.infer<typeof createDeviceSchema>;
export type UpdateDeviceInput = z.infer<typeof updateDeviceSchema>;
export type UploadBatchInput = z.infer<typeof uploadBatchSchema>;
export type ListBatchesInput = z.infer<typeof listBatchesSchema>;
export type GetStateInput = z.infer<typeof getStateSchema>;
export type ListDevicesInput = z.infer<typeof listDevicesSchema>;
export type CreatePartnerInput = z.infer<typeof createPartnerSchema>;
export type AcceptPartnerInput = z.infer<typeof acceptPartnerSchema>;
export type SetE2EEKeyInput = z.infer<typeof setE2EEKeySchema>;
export type UpdatePartnerInput = z.infer<typeof updatePartnerSchema>;
export type SettingsInput = z.infer<typeof settingsSchema>;
