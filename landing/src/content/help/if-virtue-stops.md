---
title: 'What your partner sees if Virtue stops'
description: 'Which interruptions are reported, which are not, and how each one shows up.'
topic: protection
order: 3
---

Virtue notices most ways monitoring can stop, and reports them as entries in your log feed. A few
everyday interruptions are expected and are not treated as a problem.

## Not reported as a problem

- **Shutting down, restarting or sleeping.** Virtue can tell the device was off or asleep, so
  the gap is excused. The device may show **Offline** to partners until it is back.
- **Signing out of your computer.** The gap is excused the same way.
- **Losing your internet connection.** Activity is kept on the device and uploaded once you are
  back online.

## Reported to partners

| What happened                                          | What partners see                                                                                                                                                             |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| You stopped or paused monitoring in the app            | A **Monitoring Stopped by User** entry, emailed to them straight away                                                                                                         |
| You started monitoring again                           | A **Monitoring Resumed by User** entry                                                                                                                                        |
| Virtue was closed, killed or frozen for a while        | A **Screenshot Missed** entry once it runs again                                                                                                                              |
| Virtue was killed over and over                        | A **Repeated Restarts** entry, emailed to them straight away                                                                                                                  |
| The screen capture permission was turned off           | A **Capture Failed** entry after several failed screenshots                                                                                                                   |
| The device's clock was changed                         | An **Alert** entry saying how far off the clock was                                                                                                                           |
| You signed out of the Virtue app                       | The device shows **Deactivated**, and partners are emailed                                                                                                                    |
| Virtue was uninstalled, or stopped and never restarted | Nothing more can be sent. The device shows **Offline**, and their summary email names it. On a Mac, uninstalling starts with stopping monitoring, which is reported as above. |

Each entry is explained in [log types](/help/web/log-types), and each device status in
[device status](/help/web/device-status).

## iPhone and iPad

Apple pauses Virtue's Safari extension without warning, so gaps on an iPhone or iPad do not
mean anything and are not reported. Stopping monitoring in the app and capture failures are
still reported.
