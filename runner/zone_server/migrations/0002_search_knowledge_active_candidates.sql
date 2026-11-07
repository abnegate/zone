CREATE OR REPLACE FUNCTION search_knowledge(query_vector vector, p_workspace_id uuid, p_limit integer DEFAULT 10, p_threshold double precision DEFAULT 0.7) RETURNS TABLE(entry_id uuid, similarity double precision, title text, content text, category text, tags text[])
    LANGUAGE plpgsql
    AS $$
BEGIN
  RETURN QUERY
  WITH ann AS (
    SELECT ke.id, ke.title, ke.content, ke.category, ke.tags, ke_embed.vector
    FROM knowledge_embeddings ke_embed
    JOIN knowledge_entries ke ON ke.id = ke_embed.knowledge_entry_id
    WHERE ke_embed.workspace_id = p_workspace_id
      AND ke.is_active = TRUE
    ORDER BY ke_embed.vector_bit <~> binary_quantize(query_vector)::bit(1024)
    LIMIT GREATEST(p_limit * 8, 32)
  )
  SELECT ann.id as entry_id, (1 - (ann.vector <=> query_vector))::FLOAT as similarity,
         ann.title, ann.content, ann.category, ann.tags
  FROM ann
  WHERE (1 - (ann.vector <=> query_vector)) >= p_threshold
  ORDER BY ann.vector <=> query_vector
  LIMIT p_limit;
END;
$$;
