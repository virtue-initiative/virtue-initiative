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
  avg_interval_seconds: z.number().int().positive().optional().default(300),
});

export const listDevicesSchema = z.object({
  user: z.string().min(1).optional(),
});

export const updateDeviceSchema = z
  .object({
    name: z.string().min(1).optional(),
    interval_seconds: z.number().int().positive().optional(),
    enabled: z.boolean().optional(),
  })
  .refine((data) => Object.keys(data).length > 0, {
    message: 'No fields to update',
  });

// Image schemas
export const uploadImageSchema = z.object({
  device_id: z.string().min(1),
  sha256: z.string().regex(/^[0-9a-f]{64}$/, 'Must be a valid SHA-256 hex string'),
  taken_at: z.iso.datetime(),
});

// Log schemas
export const createLogSchema = z.object({
  type: z.string().min(1),
  device_id: z.string().min(1),
  image_id: z.string().min(1).nullable().optional(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});

export const listLogsSchema = z.object({
  device_id: z.string().min(1).optional(),
  type: z.string().optional(),
  user: z.string().optional(),
  cursor: z.string().optional(),
  limit: z.coerce.number().int().positive().max(100).optional().default(50),
});

// Partner schemas
export const createPartnerSchema = z.object({
  email: z.string().email(),
  permissions: z
    .object({
      view_images: z.boolean().optional().default(true),
      view_logs: z.boolean().optional().default(true),
    })
    .optional()
    .default({ view_images: true, view_logs: true }),
});

export const acceptPartnerSchema = z.object({
  id: z.string().min(1),
});

export const updatePartnerSchema = z.object({
  permissions: z.object({
    view_images: z.boolean().optional(),
    view_logs: z.boolean().optional(),
  }),
});

// Settings schemas
export const settingsSchema = z.object({
  name: z.string().optional(),
  timezone: z.string().optional(),
  retention_days: z.number().int().positive().optional(),
});

export type SignupInput = z.infer<typeof signupSchema>;
export type LoginInput = z.infer<typeof loginSchema>;
export type CreateDeviceInput = z.infer<typeof createDeviceSchema>;
export type UpdateDeviceInput = z.infer<typeof updateDeviceSchema>;
export type UploadImageInput = z.infer<typeof uploadImageSchema>;
export type CreateLogInput = z.infer<typeof createLogSchema>;
export type ListLogsInput = z.infer<typeof listLogsSchema>;
export type ListDevicesInput = z.infer<typeof listDevicesSchema>;
export type CreatePartnerInput = z.infer<typeof createPartnerSchema>;
export type AcceptPartnerInput = z.infer<typeof acceptPartnerSchema>;
export type UpdatePartnerInput = z.infer<typeof updatePartnerSchema>;
export type SettingsInput = z.infer<typeof settingsSchema>;
