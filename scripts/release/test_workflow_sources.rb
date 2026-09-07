#!/usr/bin/env ruby
# frozen_string_literal: true

require "yaml"

ACTION = %r{\A[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*@[0-9a-f]{40}\z}
IMAGE = /\A\S+@sha256:[0-9a-f]{64}\z/

def require_mapping(value, label, errors)
  return value if value.is_a?(Hash)

  errors << "#{label} must be a mapping"
  {}
end

def validate_action(value, label, errors)
  unless value.is_a?(String)
    errors << "#{label} must be a string"
    return
  end
  return if value.start_with?("./")
  return if ACTION.match?(value)

  errors << "#{label} external action must end in a full lowercase Git SHA: #{value.inspect}"
end

def validate_image(value, label, errors)
  return if value.is_a?(String) && IMAGE.match?(value)

  errors << "#{label} must end in a full lowercase sha256 digest: #{value.inspect}"
end

def workflow_source_errors(document, name)
  errors = []
  root = require_mapping(document, name, errors)
  jobs = require_mapping(root["jobs"], "#{name}.jobs", errors)
  jobs.each do |job_name, raw_job|
    job = require_mapping(raw_job, "#{name}.jobs.#{job_name}", errors)
    validate_action(job["uses"], "#{name}.jobs.#{job_name}.uses", errors) if job.key?("uses")

    if job.key?("container")
      container = job["container"]
      if container.is_a?(Hash)
        validate_image(container["image"], "#{name}.jobs.#{job_name}.container.image", errors)
      else
        validate_image(container, "#{name}.jobs.#{job_name}.container", errors)
      end
    end

    if job.key?("services")
      services = require_mapping(job["services"], "#{name}.jobs.#{job_name}.services", errors)
      services.each do |service_name, raw_service|
        service = require_mapping(
          raw_service,
          "#{name}.jobs.#{job_name}.services.#{service_name}",
          errors
        )
        validate_image(
          service["image"],
          "#{name}.jobs.#{job_name}.services.#{service_name}.image",
          errors
        )
      end
    end

    next unless job.key?("steps")

    unless job["steps"].is_a?(Array)
      errors << "#{name}.jobs.#{job_name}.steps must be an array"
      next
    end
    job["steps"].each_with_index do |raw_step, index|
      step = require_mapping(raw_step, "#{name}.jobs.#{job_name}.steps[#{index}]", errors)
      validate_action(
        step["uses"],
        "#{name}.jobs.#{job_name}.steps[#{index}].uses",
        errors
      ) if step.key?("uses")
    end
  end
  errors
end

def parse_workflow(source, name)
  YAML.safe_load(source, aliases: false) || raise("#{name} is empty")
rescue Psych::Exception => error
  raise "#{name} is not safe YAML: #{error.message}"
end

def assert_fixture_policy
  sha = "a" * 40
  digest = "b" * 64
  valid = parse_workflow(<<~YAML, "valid fixture")
    jobs:
      test:
        container: { image: "registry.example/zephium@sha256:#{digest}" }
        steps: [{ uses: "owner/action@#{sha}" }, { uses: "./local/action" }]
  YAML
  raise "valid immutable-source fixture was rejected" unless workflow_source_errors(valid, "valid").empty?

  invalid = parse_workflow(<<~YAML, "invalid fixture")
    jobs:
      test:
        container: registry.example/zephium:latest
        services: { database: { image: "postgres:18" } }
        steps: [{ "uses": "owner/action@v4" }]
  YAML
  errors = workflow_source_errors(invalid, "invalid")
  raise "mutable-source fixture did not produce exactly three errors: #{errors.inspect}" unless errors.length == 3
end

def named_step(document, job_name, step_name)
  job = document.fetch("jobs").fetch(job_name)
  steps = job.fetch("steps")
  index = steps.index { |step| step["name"] == step_name }
  raise "#{job_name} is missing #{step_name.inspect}" unless index

  [index, steps.fetch(index)]
end

def assert_ci_dependency_staging(root)
  ci = parse_workflow(
    File.read(File.join(root, ".github/workflows/ci.yml"), encoding: "UTF-8"),
    ".github/workflows/ci.yml"
  )

  [
    [
      "blocker-security-fork",
      "Fetch exact locked blocker graphs",
      "Check the complete blocker security-fork boundary"
    ],
    [
      "rust",
      "Fetch exact locked security-fork graphs",
      "Enforce vendored security-fork sources and fixture provenance"
    ]
  ].each do |job_name, fetch_name, gate_name|
    fetch_index, fetch = named_step(ci, job_name, fetch_name)
    gate_index, = named_step(ci, job_name, gate_name)
    raise "#{job_name} must fetch before its offline provenance gate" unless fetch_index < gate_index

    fetch_run = fetch.fetch("run")
    unless fetch_run.include?("cargo fetch --locked\n")
      raise "#{job_name} locked prefetch omits the complete root lock"
    end
    %w[
      x86_64-pc-windows-msvc
      x86_64-apple-darwin
      aarch64-apple-darwin
      x86_64-unknown-linux-gnu
      vendor/adblock/Cargo.toml
    ].each do |required|
      raise "#{job_name} locked prefetch omits #{required}" unless fetch_run.include?(required)
    end
  end

  linux_fetch_index, linux_fetch = named_step(
    ci,
    "linux-native-security",
    "Fetch exact locked Linux blocker graph"
  )
  linux_gate_index, = named_step(
    ci,
    "linux-native-security",
    "Prove the exact bundled blocker seed compiles in native WebKitGTK"
  )
  linux_fetch_run = linux_fetch.fetch("run")
  unless linux_fetch_index < linux_gate_index &&
         linux_fetch_run.include?("cargo fetch --locked\n") &&
         linux_fetch_run.include?("cargo fetch --locked --target x86_64-unknown-linux-gnu")
    raise "Linux native blocker proof must stage its exact locked graph before offline materialization"
  end

  fuzz_steps = ci.fetch("jobs").fetch("blocker-fuzz-smoke").fetch("steps")
  installer = fuzz_steps.find do |step|
    step["uses"] == "taiki-e/install-action@43aecc8d72668fbcfe75c31400bc4f890f1c5853"
  end
  raise "blocker fuzz smoke is missing its pinned tool installer" unless installer
  unless installer.fetch("with").fetch("tool") == "cargo-deny@0.20.2" &&
         installer.fetch("with").fetch("fallback") == "none"
    raise "blocker fuzz binary installer must remain exact and fallback-free"
  end
  _, cargo_fuzz = named_step(
    ci,
    "blocker-fuzz-smoke",
    "Install exact cargo-fuzz toolchain"
  )
  unless cargo_fuzz.fetch("run") == "cargo install --locked --version 0.13.2 cargo-fuzz"
    raise "cargo-fuzz must be built from the exact crates.io release lockfile"
  end
end

def assert_macos_runtime_evidence_boundary(root)
  ci = parse_workflow(
    File.read(File.join(root, ".github/workflows/ci.yml"), encoding: "UTF-8"),
    ".github/workflows/ci.yml"
  )
  source_if = "runner.os == 'macOS'"
  release_if = "runner.os == 'macOS' && inputs.checkout_ref != ''"
  ordinary_if = "runner.os == 'macOS' && inputs.checkout_ref == ''"

  _, source = named_step(
    ci,
    "rust-platforms",
    "Keep macOS native security probe graphs warning-clean"
  )
  raise "macOS native probe source gate must run on every macOS job" unless source.fetch("if") == source_if
  %w[
    macos-principal-isolation-probe
    macos-web-extension-probe
    macos-web-extension-resource-probe
  ].each do |binary|
    raise "macOS native probe source gate omits #{binary}" unless source.fetch("run").include?(binary)
  end

  _, notice = named_step(ci, "rust-platforms", "Record hosted macOS runtime-evidence boundary")
  unless notice.fetch("if") == ordinary_if &&
         notice.fetch("run").include?("mint no native runtime evidence")
    raise "ordinary hosted macOS CI must disclose that it mints no native evidence"
  end

  [
    "Prove macOS principal worlds and handlers are mutually isolated",
    "Prove public macOS WKWebExtension admission and isolation",
    "Classify macOS extension resource transport"
  ].each do |step_name|
    _, step = named_step(ci, "rust-platforms", step_name)
    raise "#{step_name} must remain release-call-only" unless step.fetch("if") == release_if
  end

  desktop = File.read(File.join(root, "desktop/src/platform/macos.rs"), encoding: "UTF-8")
  ignore = "requires a release-qualified system Safari/WebKit pair; the explicit native security probe owns this environment-dependent gate"
  unless desktop.include?(%[#[ignore = "#{ignore}"]\n    fn system_safari_matches_the_framework_owning_wkwebview()])
    raise "the environment-dependent Safari/WebKit test must stay captive to the explicit native release probe"
  end
end

def linux_native_environment_errors(ci, dockerfile, launcher, preflight, profile)
  errors = []
  job = ci.fetch("jobs").fetch("linux-native-security")
  errors << "native gates must use the hosted VM, not an implicit Docker job" unless
    job["runs-on"] == "ubuntu-24.04" && !job.key?("container")
  errors << "native steps must stay inside their exact unprivileged executor" unless
    job.fetch("defaults").fetch("run")["shell"] == "bash scripts/ci/linux_native_container.sh exec {0}"
  ordered = [
    "Start capability-free Fedora native test environment",
    "Fetch exact locked Linux blocker graph",
    "Seal native test network after dependency acquisition",
    "Prove unprivileged native sandbox prerequisites",
    "Prove native WebKitGTK sandbox package prerequisites",
    "Prove the exact bundled blocker seed compiles in native WebKitGTK",
    "Prove Wry WebKitWebProcess confinement"
  ]
  indices = ordered.map { |name| named_step(ci, "linux-native-security", name).first }
  errors << "native acquisition, network seal and proofs are out of order" unless indices == indices.sort
  host_steps = {
    ordered[0] => "start",
    ordered[2] => "seal",
    "Retire exact Fedora native test environment" => "stop"
  }
  job.fetch("steps").each do |step|
    next unless step.key?("run")

    mode = host_steps[step["name"]]
    if mode
      errors << "host helper mode changed" unless step["shell"] == "bash" &&
        step["run"] == "bash scripts/ci/linux_native_container.sh #{mode}"
    elsif step.key?("shell") || step.key?("container") || step.key?("continue-on-error") || step.key?("if")
      errors << "native proof escaped or bypassed the exact executor"
    end
  end
  _, cleanup = named_step(ci, "linux-native-security", host_steps.keys.last)
  errors << "native environment cleanup must be unconditional" unless cleanup["if"] == "always()"
  validate_image(dockerfile[/^FROM (.+)$/, 1], "Fedora native Dockerfile", errors)
  errors << "native image must stay unprivileged and exact-toolchain-bound" unless
    dockerfile.include?("USER 10001:10001") && dockerfile.include?("RUSTUP_TOOLCHAIN=1.95.0")
  [
    "--user 10001:10001 --cap-drop ALL --security-opt no-new-privileges",
    "--security-opt seccomp=unconfined --security-opt apparmor=zephium-native-ci",
    "--read-only --pids-limit 2048", "target=/workspace,readonly",
    "docker exec --interactive --user 10001:10001",
    "target=/opt/rustup,readonly", "target=/opt/rust-bin,readonly",
    'docker network disconnect bridge "${native_name}"',
    'CARGO_NET_OFFLINE=${native_offline}',
    'sudo apparmor_parser --remove scripts/ci/linux-native.apparmor'
  ].each do |required|
    errors << "native launcher lost #{required}" unless launcher.include?(required)
  end
  errors << "native launcher must mount only its four reviewed paths" unless launcher.scan(/--mount\b/).length == 4
  ["--privileged", "--cap-add", "--device", "--pid host", "--network host",
   "docker.sock", "GITHUB_TOKEN", "sysctl -w", "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS"].each do |forbidden|
    errors << "native launcher adds forbidden authority #{forbidden}" if launcher.include?(forbidden)
  end
  ["CapInh CapPrm CapEff CapBnd CapAmb", "NoNewPrivs:", "Seccomp_filters:",
   "--unshare-user --unshare-pid --unshare-net", '"${native_interface##*/}" = lo',
   '"$(id -u)" = 10001'].each do |required|
    errors << "native preflight lost #{required}" unless preflight.include?(required)
  end
  declarations = profile.lines.reject { |line| line.start_with?("#") || line.strip.empty? }.join
  unless declarations == "abi <abi/4.0>,\nprofile zephium-native-ci flags=(unconfined) {\n  userns,\n}\n"
    errors << "scoped native userns profile changed"
  end
  errors
end

def assert_linux_native_environment(root)
  ci = parse_workflow(File.read(File.join(root, ".github/workflows/ci.yml")), "CI")
  paths = %w[linux-native.Dockerfile linux_native_container.sh prove_linux_native_namespaces.sh linux-native.apparmor]
  sources = paths.map { |path| File.read(File.join(root, "scripts/ci", path), encoding: "UTF-8") }
  errors = linux_native_environment_errors(ci, *sources)
  raise errors.join("\n") unless errors.empty?

  [
    [0, "@sha256:", "@mutable:"],
    [0, "USER 10001:10001", "USER 0:0"],
    [1, "--cap-drop ALL", "--cap-add SYS_ADMIN"],
    [1, "target=/workspace,readonly", "target=/workspace"],
    [1, "docker network disconnect bridge", "echo skipped"],
    [1, "--security-opt no-new-privileges", "--privileged"],
    [2, "--unshare-user", "--unshare-user-try"],
    [2, "Seccomp_filters:", "Unrelated:"],
    [3, "zephium-native-ci", "unconfined"]
  ].each do |index, before, after|
    mutation = sources.dup
    raise "missing Linux policy mutation target #{before}" unless mutation[index].include?(before)
    mutation[index] = mutation[index].sub(before, after)
    if linux_native_environment_errors(ci, *mutation).empty?
      raise "Linux native authority mutation escaped: #{before}"
    end
  end
  escaped = Marshal.load(Marshal.dump(ci))
  _, gate = named_step(escaped, "linux-native-security", "Prove Wry WebKitWebProcess confinement")
  gate["shell"] = "bash"
  raise "native gate escaped onto the host" if linux_native_environment_errors(escaped, *sources).empty?
end

assert_fixture_policy
root = File.expand_path("../..", __dir__)
assert_ci_dependency_staging(root)
assert_macos_runtime_evidence_boundary(root)
assert_linux_native_environment(root)
workflows = Dir[File.join(root, ".github/workflows/*.{yml,yaml}")].sort
raise "repository contains no GitHub Actions workflows" if workflows.empty?

errors = workflows.flat_map do |path|
  name = path.delete_prefix("#{root}/")
  workflow_source_errors(parse_workflow(File.read(path, encoding: "UTF-8"), name), name)
end
unless errors.empty?
  warn errors.join("\n")
  exit 1
end

puts "workflow source policy: #{workflows.length} workflows verified"
