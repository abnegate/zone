# frozen_string_literal: true

class ReleaseVersion
  PATTERN = /\A(\d+)\.(\d+)\.(\d+)(?:[-+].*)?\z/

  def self.parts(version)
    match = PATTERN.match(version.to_s.delete_prefix('v'))
    raise ArgumentError, "Version #{version.inspect} is not semver" unless match

    match.captures.map { |part| Integer(part, 10) }
  end

  def self.marketing(version)
    parts(version).join('.')
  end

  def self.code(version)
    major, minor, patch = parts(version)
    (major * 1_000_000) + (minor * 1_000) + patch
  end
end
