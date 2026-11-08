# frozen_string_literal: true

require_relative '../release_version_code'

def assert_equal(expected, actual)
  return if expected == actual

  raise "Expected #{expected.inspect}, got #{actual.inspect}"
end

def assert_nil(actual)
  return if actual.nil?

  raise "Expected nil, got #{actual.inspect}"
end

def assert_raises(error)
  yield
rescue error
  nil
else
  raise "Expected #{error}"
end

assert_equal '1.2.3', ReleaseVersion.marketing('v1.2.3-rc.1')
assert_equal 1_002_003, ReleaseVersion.code('1.2.3+build.4')
assert_raises(ArgumentError) { ReleaseVersion.code('1.2') }

messages = []
result = ReleaseVersionCode.next_code(
  version: '0.2.0',
  code_fetcher: lambda do |track|
    raise StandardError, 'track missing' if track == 'alpha'

    []
  end,
  logger: ->(message) { messages << message },
)
assert_equal 2_000, result.code
assert_nil result.remote_max
assert_equal ['Unable to query Play track alpha version codes: track missing.'], messages

result = ReleaseVersionCode.next_code(
  version: '0.2.0-rc.2',
  code_fetcher: ->(track) { track == 'internal' ? [2_000] : [] },
)
assert_equal 2_001, result.code
assert_equal 2_000, result.remote_max

result = ReleaseVersionCode.next_code(
  version: '0.3.0',
  code_fetcher: ->(_track) { [2_001] },
)
assert_equal 3_000, result.code

puts 'release_version_code_test passed'
