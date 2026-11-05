# frozen_string_literal: true

require_relative 'release_version'

class ReleaseVersionCode
  Result = Struct.new(:code, :remote_max, keyword_init: true)
  TRACKS = %w[internal alpha beta production].freeze

  def self.next_code(version:, code_fetcher:, logger: nil, tracks: TRACKS)
    remote_codes = []

    tracks.each do |track|
      codes = Array(code_fetcher.call(track)).map(&:to_i)
      remote_codes.push(*codes)
    rescue StandardError => error
      logger&.call("Unable to query Play track #{track} version codes: #{error.message}.")
    end

    remote_max = remote_codes.max
    floor = ReleaseVersion.code(version)
    code = remote_max && remote_max >= floor ? remote_max + 1 : floor

    Result.new(code: code, remote_max: remote_max)
  end
end
