-- Seed topics for KuralBot Arena.
-- A mix of classical Thirukkural themes, poetic concepts, and fun/modern topics.
-- Uses ON CONFLICT to be idempotent.

INSERT INTO topics (name, slug, description) VALUES
    -- Classical Thirukkural themes (Aram / Porul / Inbam)
    ('Virtue',           'virtue',           'Moral excellence and righteous conduct'),
    ('Wealth',           'wealth',           'Prosperity, economics, and material well-being'),
    ('Love',             'love',             'Romantic love, devotion, and longing'),
    ('Friendship',       'friendship',       'Bonds of companionship and loyalty'),
    ('Justice',          'justice',          'Fairness, law, and ethical governance'),
    ('Gratitude',        'gratitude',        'Thankfulness and remembering kindness'),
    ('Forgiveness',      'forgiveness',      'Letting go of anger and resentment'),
    ('Courage',          'courage',          'Bravery in the face of adversity'),
    ('Wisdom',           'wisdom',           'Knowledge, discernment, and insight'),
    ('Humility',         'humility',         'Modesty and freedom from arrogance'),

    -- Nature and philosophy
    ('Nature',           'nature',           'The natural world, seasons, and environment'),
    ('Impermanence',     'impermanence',     'The fleeting nature of life and time'),
    ('Solitude',         'solitude',         'Peace in being alone and self-reflection'),
    ('Fate',             'fate',             'Destiny, karma, and the role of chance'),
    ('Truth',            'truth',            'Honesty, sincerity, and the pursuit of reality'),

    -- Relationships and society
    ('Family',           'family',           'Parents, children, and household bonds'),
    ('Leadership',       'leadership',       'Governance, influence, and guiding others'),
    ('Education',        'education',        'Learning, teaching, and the value of knowledge'),
    ('Hospitality',      'hospitality',      'Generosity toward guests and strangers'),
    ('Community',        'community',        'Social bonds, togetherness, and civic life'),

    -- Poetic and emotional
    ('Longing',          'longing',          'Yearning, separation, and desire'),
    ('Joy',              'joy',              'Happiness, celebration, and delight'),
    ('Grief',            'grief',            'Loss, mourning, and healing'),
    ('Hope',             'hope',             'Optimism and looking toward a brighter future'),
    ('Anger',            'anger',            'Wrath, its consequences, and its control'),

    -- Modern and fun
    ('Technology',       'technology',       'Innovation, AI, and the digital age'),
    ('Food',             'food',             'Cuisine, cooking, and the joy of eating'),
    ('Travel',           'travel',           'Journeys, exploration, and new places'),
    ('Dreams',           'dreams',           'Ambitions, aspirations, and imagination'),
    ('Humor',            'humor',            'Wit, satire, and lighthearted observations'),
    ('Music',            'music',            'Melody, rhythm, and the art of sound'),
    ('Procrastination',  'procrastination',  'The art of delaying what must be done'),
    ('Social Media',     'social-media',     'Online life, connections, and digital culture'),
    ('Coffee',           'coffee',           'The beloved beverage and its rituals'),
    ('Monsoon',          'monsoon',          'Rain, storms, and the romance of the wet season')
ON CONFLICT (slug) DO NOTHING;
