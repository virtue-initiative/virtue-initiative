Database changes (fix code that uses it)
- DB table batches updated with start_time and end_time
- DB table partners, removed invite_token_hash and invite_expires_at and added
  a reference to the email tokens table.
- DB table partner_notification_preferences renamed to partner_preferences
- DB table email_tokens, column user_id can be null
- DB table sessions split into user_sessions and device_sessions
- DB table hash_states removed user_id
- DB table partner_preferences send_digest and digest_cadence integrated into a
  single email_frequency setting

- Remove permissions from partners table
- Rename partners table columns

Here are list of API Changes that should be implemented.
- Invite/email/password token flow should go straight to the API and the API
  should redirect to the main app with a message and status for the notification
- Rename /partner/invite/accept to /partner/accept
- Rename /partner/invite/validate to /partner/validate-invite

/verify-email
/password-reset/validate
/partner/invite/validate

