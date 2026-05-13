import * as preact from "preact";
import { useRef, useState } from "preact/hooks";
import {
  Alert,
  Avatar,
  Badge,
  Button,
  Card,
  CardActions,
  CardGrid,
  CardHeader,
  Checkbox,
  Dialog,
  DialogActions,
  DialogHeader,
  Field,
  IconButton,
  Input,
  Menu,
  Radio,
  SegmentedControl,
  Select,
  Skeleton,
  Spinner,
  Tabs,
  Textarea,
  Tooltip,
  ToastProvider,
  useToast,
} from "@virtueinitiative/shared-web";
import "@virtueinitiative/shared-web/index.css";
import "./Components.css";

function ColorSwatch({
  name,
  variable,
}: {
  name: string;
  variable: string;
}) {
  return (
    <div class="dev-swatch">
      <div
        class="dev-swatch__color"
        style={{ background: `var(${variable})` }}
      />
      <div class="dev-swatch__info">
        <code class="dev-swatch__var">{variable}</code>
        <span class="dev-swatch__name">{name}</span>
      </div>
    </div>
  );
}

function Section({
  title,
  children,
}: {
  title: string;
  children: preact.ComponentChildren;
}) {
  return (
    <section class="dev-section">
      <h2 class="dev-section__title">{title}</h2>
      <div class="dev-section__content">{children}</div>
    </section>
  );
}

function Row({ children }: { children: preact.ComponentChildren }) {
  return <div class="dev-row">{children}</div>;
}

function ToastDemo() {
  const { push } = useToast();
  return (
    <Row>
      <Button variant="primary" onClick={() => push("Success message!", "success")}>
        Success Toast
      </Button>
      <Button variant="ghost" onClick={() => push("Something went wrong.", "error")}>
        Error Toast
      </Button>
      <Button variant="outline" onClick={() => push("Here is some info.", "info")}>
        Info Toast
      </Button>
      <Button
        variant="flat"
        onClick={() =>
          push("This stays until dismissed.", "info", {
            durationMs: null,
            dismissible: true,
          })
        }
      >
        Persistent Toast
      </Button>
    </Row>
  );
}

function DialogDemo() {
  const ref = useRef<HTMLDialogElement>(null);

  return (
    <div>
      <Button
        variant="primary"
        onClick={() => {
          if (ref.current) ref.current.showModal();
        }}
      >
        Open Dialog
      </Button>
      <Dialog dialogRef={ref}>
        <DialogHeader>Example Dialog</DialogHeader>
        <p style={{ margin: "0 0 1rem" }}>
          This is the shared-web Dialog component with vi-* classes.
        </p>
        <DialogActions>
          <Button
            variant="ghost"
            onClick={() => ref.current?.close()}
          >
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => ref.current?.close()}
          >
            Confirm
          </Button>
        </DialogActions>
      </Dialog>
    </div>
  );
}

export function ComponentsPage() {
  const [tab, setTab] = useState("buttons");
  const [seg, setSeg] = useState("one");
  const [checkboxChecked, setCheckboxChecked] = useState(false);
  const [radioVal, setRadioVal] = useState("a");
  const [inputVal, setInputVal] = useState("");
  const [selectVal, setSelectVal] = useState("");

  return (
    <ToastProvider>
      <div class="dev-page">
        <header class="dev-header">
          <h1>Component Library Preview</h1>
          <p class="dev-subtitle">
            All vi-* namespaced components — light &amp; dark via system
            preference or{" "}
            <code>[data-theme]</code>
          </p>
        </header>

        <Section title="Token Palette — Colors">
          <div class="dev-swatches">
            <ColorSwatch name="bg" variable="--bg" />
            <ColorSwatch name="surface" variable="--surface" />
            <ColorSwatch name="bg-subtle" variable="--bg-subtle" />
            <ColorSwatch name="border" variable="--border" />
            <ColorSwatch name="text" variable="--text" />
            <ColorSwatch name="text-muted" variable="--text-muted" />
            <ColorSwatch name="accent" variable="--accent" />
            <ColorSwatch name="danger" variable="--danger" />
            <ColorSwatch name="warning" variable="--warning" />
          </div>
        </Section>

        <Section title="Token Palette — Spacing">
          <div class="dev-spacing-grid">
            {[1, 2, 3, 4, 5, 6, 7, 8].map((n) => (
              <div key={n} class="dev-spacing-item">
                <div
                  class="dev-spacing-bar"
                  style={{
                    width: `var(--space-${n})`,
                    height: "1rem",
                    background: "var(--accent)",
                  }}
                />
                <code>--space-{n}</code>
              </div>
            ))}
          </div>
        </Section>

        <Section title="Button">
          <Row>
            <Button variant="primary">Primary</Button>
            <Button variant="outline">Outline</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="danger">Danger</Button>
            <Button variant="flat">Flat</Button>
          </Row>
          <Row>
            <Button variant="primary" size="sm">
              Primary SM
            </Button>
            <Button variant="outline" size="sm">
              Outline SM
            </Button>
            <Button variant="ghost" size="sm">
              Ghost SM
            </Button>
            <Button variant="danger" size="sm">
              Danger SM
            </Button>
          </Row>
          <Row>
            <Button variant="primary" disabled>
              Disabled
            </Button>
            <Button variant="ghost" disabled>
              Disabled
            </Button>
          </Row>
        </Section>

        <Section title="IconButton">
          <Row>
            <IconButton>
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <path d="M12 5v14M5 12h14" />
              </svg>
            </IconButton>
            <IconButton isActive>
              <svg
                width="16"
                height="16"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <path d="M5 12h14" />
              </svg>
            </IconButton>
          </Row>
        </Section>

        <Section title="Card">
          <CardGrid>
            <Card>
              <CardHeader>
                <strong>Basic Card</strong>
                <Badge variant="green">Active</Badge>
              </CardHeader>
              <p style={{ margin: 0, color: "var(--text-muted)", fontSize: "var(--text-sm)" }}>
                Card content here. Uses vi-card, vi-card-header, vi-card-actions.
              </p>
              <CardActions>
                <Button variant="ghost" size="sm">
                  Cancel
                </Button>
                <Button variant="primary" size="sm">
                  Action
                </Button>
              </CardActions>
            </Card>
            <Card highlight>
              <CardHeader>
                <strong>Highlighted Card</strong>
                <Badge variant="yellow">Pending</Badge>
              </CardHeader>
              <p style={{ margin: 0, color: "var(--text-muted)", fontSize: "var(--text-sm)" }}>
                This card has a colored border via vi-card--highlight.
              </p>
            </Card>
          </CardGrid>
        </Section>

        <Section title="Field + Input + Textarea + Select">
          <div class="dev-form-grid">
            <Field label="Email address" helpText="We'll never share your email.">
              <Input
                type="email"
                placeholder="you@example.com"
                value={inputVal}
                onInput={(e) =>
                  setInputVal((e.target as HTMLInputElement).value)
                }
              />
            </Field>
            <Field label="Bio">
              <Textarea placeholder="Tell us about yourself..." />
            </Field>
            <Field label="Country">
              <Select
                value={selectVal}
                onChange={(e) =>
                  setSelectVal((e.target as HTMLSelectElement).value)
                }
              >
                <option value="">Select country…</option>
                <option value="us">United States</option>
                <option value="uk">United Kingdom</option>
                <option value="ca">Canada</option>
              </Select>
            </Field>
            <Field
              label="Username"
              error="Username is already taken."
            >
              <Input
                type="text"
                placeholder="username"
                error
                value="taken_user"
              />
            </Field>
          </div>
        </Section>

        <Section title="Checkbox + Radio">
          <Row>
            <Checkbox
              label="Accept terms and conditions"
              checked={checkboxChecked}
              onChange={(e) =>
                setCheckboxChecked((e.target as HTMLInputElement).checked)
              }
            />
          </Row>
          <Row>
            <Radio
              label="Option A"
              name="radio-demo"
              value="a"
              checked={radioVal === "a"}
              onChange={() => setRadioVal("a")}
            />
            <Radio
              label="Option B"
              name="radio-demo"
              value="b"
              checked={radioVal === "b"}
              onChange={() => setRadioVal("b")}
            />
            <Radio
              label="Option C"
              name="radio-demo"
              value="c"
              checked={radioVal === "c"}
              onChange={() => setRadioVal("c")}
            />
          </Row>
        </Section>

        <Section title="Alert">
          <div class="dev-stack">
            <Alert variant="success">
              Your changes have been saved successfully.
            </Alert>
            <Alert variant="error">
              Something went wrong. Please try again.
            </Alert>
            <Alert variant="warning">
              Your subscription is expiring soon.
            </Alert>
            <Alert variant="info">
              New features are available. Check out the changelog.
            </Alert>
          </div>
        </Section>

        <Section title="Badge">
          <Row>
            <Badge variant="green">Active</Badge>
            <Badge variant="gray">Inactive</Badge>
            <Badge variant="yellow">Pending</Badge>
            <Badge variant="red">Error</Badge>
          </Row>
        </Section>

        <Section title="Tabs">
          <Tabs
            tabs={[
              { label: "Overview", value: "overview" },
              { label: "Details", value: "details" },
              { label: "History", value: "history" },
            ]}
            value={tab}
            onChange={setTab}
          />
          <p style={{ marginTop: "1rem", color: "var(--text-muted)" }}>
            Active tab: <strong>{tab}</strong>
          </p>
        </Section>

        <Section title="SegmentedControl">
          <SegmentedControl
            segments={[
              { label: "One", value: "one" },
              { label: "Two", value: "two" },
              { label: "Three", value: "three" },
            ]}
            value={seg}
            onChange={setSeg}
          />
          <p style={{ marginTop: "1rem", color: "var(--text-muted)" }}>
            Active: <strong>{seg}</strong>
          </p>
        </Section>

        <Section title="Spinner">
          <Row>
            <Spinner size="sm" />
            <Spinner size="md" />
            <Spinner size="lg" />
          </Row>
        </Section>

        <Section title="Skeleton">
          <div class="dev-stack" style={{ maxWidth: 400 }}>
            <Skeleton variant="text" />
            <Skeleton variant="text" width="70%" />
            <Skeleton variant="rect" height="6rem" />
            <div style={{ display: "flex", gap: "0.75rem", alignItems: "center" }}>
              <Skeleton variant="circle" />
              <div style={{ flex: 1 }}>
                <Skeleton variant="text" />
              </div>
            </div>
          </div>
        </Section>

        <Section title="Menu">
          <Row>
            <Menu
              trigger={<Button variant="ghost">Open Menu ▾</Button>}
              items={[
                { label: "Edit", onClick: () => alert("Edit") },
                { label: "Duplicate", onClick: () => alert("Duplicate") },
                {
                  label: "Delete",
                  onClick: () => alert("Delete"),
                  danger: true,
                },
              ]}
            />
          </Row>
        </Section>

        <Section title="Tooltip">
          <Row>
            <Tooltip content="This is a tooltip!">
              <Button variant="ghost">Hover me</Button>
            </Tooltip>
            <Tooltip content="Another tooltip with longer text that wraps nicely">
              <Badge variant="green">Hover badge</Badge>
            </Tooltip>
          </Row>
        </Section>

        <Section title="Avatar">
          <Row>
            <Avatar name="Alice Smith" size="sm" />
            <Avatar name="Bob Jones" size="md" />
            <Avatar name="Charlie Brown" size="lg" />
            <Avatar size="md" />
            <Avatar
              src="https://avatars.githubusercontent.com/u/1?v=4"
              name="GitHub User"
              size="md"
            />
          </Row>
        </Section>

        <Section title="Dialog">
          <DialogDemo />
        </Section>

        <Section title="Toast">
          <ToastDemo />
        </Section>
      </div>
    </ToastProvider>
  );
}
