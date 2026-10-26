<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: { checked: boolean; label?: string; disabled?: boolean; onchange?: (v: boolean) => void } = $props();
</script>

<label class="toggle" class:disabled>
  <input
    type="checkbox"
    {disabled}
    bind:checked
    onchange={() => onchange?.(checked)}
  />
  <span class="track"><span class="knob"></span></span>
  {#if label}<span class="label">{label}</span>{/if}
</label>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
    font-size: 13px;
  }
  .toggle.disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  input {
    position: absolute;
    opacity: 0;
    pointer-events: none;
  }
  .track {
    position: relative;
    width: 38px;
    height: 20px;
    border: 1.5px solid var(--ink);
    background: var(--paper-2);
    flex: none;
  }
  .track::before {
    content: "";
    position: absolute;
    inset: 0;
    background: var(--accent);
    opacity: 0;
    transition: opacity var(--t-mid);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 13px;
    height: 13px;
    background: var(--ink);
    transition: transform var(--t-mid) var(--ease-out);
  }
  input:checked + .track::before {
    opacity: 1;
  }
  input:checked + .track .knob {
    transform: translateX(18px);
    background: var(--accent-ink);
  }
  input:focus-visible + .track {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
