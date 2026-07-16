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

assert_fixture_policy
root = File.expand_path("../..", __dir__)
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
