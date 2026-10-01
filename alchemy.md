# Print Manager Invariants

This document records the durable rules that Print Manager changes must preserve.

## INV-DEV-SERVER-PORT: Keep the monitored development server on port 3000

- **Scope:** Local development work on Print Manager.
- **Rule:** Keep the Print Manager development server available at `http://127.0.0.1:3000` while working, and verify that it responds after changes or restarts.
- **Owner:** The Print Manager local development workflow.
- **Evidence:** An HTTP request to `http://127.0.0.1:3000` succeeds after each change or server restart.
- **Origin:** The user stated that they continuously monitor this project on port 3000.

## INV-WEB-APP-INSPECTION: Inspect requested web-app changes with agent-browser

- **Scope:** Every requested change to the Print Manager web application.
- **Rule:** Before reporting a web-app change complete, inspect the affected workflow in the live port-3000 app with `agent-browser` at representative laptop, desktop, and half-monitor viewports. Phone and mobile layouts are not supported and do not require verification.
- **Owner:** The Print Manager change-verification workflow.
- **Evidence:** The affected workflow is exercised with `agent-browser` against `http://127.0.0.1:3000`, including representative laptop, desktop, and half-monitor viewport inspection and a check for browser console or runtime errors.
- **Origin:** The user explicitly required `agent-browser` inspection for every requested web-app change.

## INV-SCHEDULE-HORIZON: Keep the active schedule bounded to four dated lanes

- **Scope:** The active Print Manager job schedule.
- **Rule:** Show at most four dated lanes: a first lane combining today's and overdue jobs, followed by the next three days. Mark every overdue job with a textual `Overdue` tag, place all remaining dated and undated jobs in the far-right lane, and do not require horizontal scrolling to reach any lane.
- **Owner:** The schedule partitioning and responsive board layout.
- **Evidence:** Schedule partitioning tests cover overdue, today, the four-day boundary, later work, and undated work; browser inspection confirms all five lanes fit without horizontal overflow and overdue cards remain explicitly labeled at supported viewports.
- **Origin:** The user requested a four-day schedule with today and overdue combined, overdue tagging, remaining jobs on the far right, and no horizontal scrolling.
